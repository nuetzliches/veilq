//! What each hand-written patch of the detection is worth (#161): the
//! corpus read once with every patch on, then once per patch with it off.
//!
//! The patches are listed in `eval/patches.toml` ([`Inventory`]) with the
//! case each protects and the corpus files that hold it. A patch whose
//! switching off changes nothing is dead today, or its case is missing from
//! the corpus; one that changes something guards that, and the run names
//! where.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::check::{self, Evaluation};

/// `eval/patches.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub patch: Vec<Patch>,
}

/// One patch: where it is, what it does, why, and what guards it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// The name [`veilq_core::patch::on`] knows it by.
    pub id: String,
    /// The file and the list, function or pattern.
    #[serde(rename = "where")]
    pub place: String,
    /// What it does to a candidate.
    pub what: String,
    /// The ADRs and issues that brought it.
    #[serde(default)]
    pub origin: Vec<String>,
    /// The case it protects, a synthetic example.
    pub case: String,
    /// The corpus files that hold the case; empty when none does.
    #[serde(default)]
    pub corpus: Vec<String>,
    /// The last ablation's verdict: `guards`, `dead` (its case is in the
    /// corpus, and it changes nothing), `uncovered` (no case in the corpus)
    /// or `unmeasured`.
    #[serde(default = "unmeasured")]
    pub ablation: String,
}

fn unmeasured() -> String {
    "unmeasured".into()
}

impl Inventory {
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let inv: Self = toml::from_str(&std::fs::read_to_string(path)?)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        let mut seen = std::collections::BTreeSet::new();
        for p in &inv.patch {
            if !seen.insert(p.id.as_str()) {
                anyhow::bail!("{}: the patch {} is listed twice", path.display(), p.id);
            }
            if !matches!(
                p.ablation.as_str(),
                "guards" | "dead" | "uncovered" | "unmeasured"
            ) {
                anyhow::bail!(
                    "{}: the patch {} has the verdict {:?}; guards, dead, uncovered or unmeasured",
                    path.display(),
                    p.id,
                    p.ablation
                );
            }
            // Dead says the case was there and nothing changed; without a
            // case the patch is uncovered, not dead.
            let consistent = match p.ablation.as_str() {
                "dead" => !p.corpus.is_empty(),
                "uncovered" => p.corpus.is_empty(),
                _ => true,
            };
            if !consistent {
                anyhow::bail!(
                    "{}: the patch {} is {} with {} corpus files",
                    path.display(),
                    p.id,
                    p.ablation,
                    p.corpus.len()
                );
            }
        }
        Ok(inv)
    }
}

/// What switching one patch off changed against every patch on.
#[derive(Debug, Serialize)]
pub struct Effect {
    pub id: String,
    /// Values no longer covered, and newly covered (negative).
    pub misses: i64,
    /// False findings added, and removed (negative).
    pub false_positives: i64,
    /// The profiles where either changed, with the change of each.
    pub profiles: BTreeMap<String, (i64, i64)>,
}

impl Effect {
    pub fn verdict(&self) -> &'static str {
        if self.misses == 0 && self.false_positives == 0 && self.profiles.is_empty() {
            "dead"
        } else {
            "guards"
        }
    }
}

/// The change from `base` (every patch on) to `off`.
pub fn effect(id: &str, base: &Evaluation, off: &Evaluation) -> Effect {
    let mut profiles = BTreeMap::new();
    for name in base.per_profile.keys().chain(off.per_profile.keys()) {
        let b = base.per_profile.get(name).cloned().unwrap_or_default();
        let o = off.per_profile.get(name).cloned().unwrap_or_default();
        let misses = (b.covered as i64 - o.covered as i64) - (b.truth as i64 - o.truth as i64);
        let fp = o.false_positives as i64 - b.false_positives as i64;
        if misses != 0 || fp != 0 {
            profiles.insert(name.clone(), (misses, fp));
        }
    }
    let (b, o) = (base.total(), off.total());
    Effect {
        id: id.to_owned(),
        misses: b.covered as i64 - o.covered as i64,
        false_positives: o.false_positives as i64 - b.false_positives as i64,
        profiles,
    }
}

/// Reads the corpora `dirs` with every patch on, then once per patch of
/// `ids` with it off. The corpora are read one after another: the switch
/// holds for the whole process. With `redact` the findings are a
/// redaction's: its verify rounds read veilq's own output, where a patch
/// may matter that a scan never needs (ADR-0071).
pub fn run(
    dirs: &[std::path::PathBuf],
    ids: &[String],
    only: Option<&str>,
    ner: bool,
    ocr: bool,
    redact: bool,
) -> anyhow::Result<Vec<Effect>> {
    let read = |off: &[String]| -> anyhow::Result<Evaluation> {
        veilq_core::patch::switch_off(off)?;
        let mut all = Evaluation::default();
        for d in dirs {
            let (e, _) = check::evaluate(d, only, ner, ocr, redact, &Default::default())?;
            merge(&mut all, e);
        }
        Ok(all)
    };
    let base = read(&[])?;
    let mut out = Vec::new();
    for id in ids {
        let off = read(std::slice::from_ref(id))?;
        out.push(effect(id, &base, &off));
    }
    veilq_core::patch::switch_off(&[])?;
    Ok(out)
}

/// Adds `e` to `all`: the counts, not the lists.
fn merge(all: &mut Evaluation, e: Evaluation) {
    all.files += e.files;
    for (k, s) in e.per_entity {
        let a = all.per_entity.entry(k).or_default();
        a.truth += s.truth;
        a.covered += s.covered;
        a.typed += s.typed;
        a.false_positives += s.false_positives;
    }
    for (k, p) in e.per_profile {
        let a = all.per_profile.entry(k).or_default();
        a.files += p.files;
        a.truth += p.truth;
        a.covered += p.covered;
        a.findings += p.findings;
        a.false_positives += p.false_positives;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::ProfileStat;

    fn eval(covered: usize, fp: usize) -> Evaluation {
        let mut e = Evaluation::default();
        e.per_entity.insert(
            "PERSON".into(),
            check::Stat {
                truth: 4,
                covered,
                typed: covered,
                false_positives: fp,
            },
        );
        e.per_profile.insert(
            "de/png/news".into(),
            ProfileStat {
                files: 1,
                truth: 4,
                covered,
                findings: covered + fp,
                false_positives: fp,
            },
        );
        e
    }

    #[test]
    fn a_patch_that_changes_nothing_is_dead() {
        let e = effect("x", &eval(4, 1), &eval(4, 1));
        assert_eq!(e.verdict(), "dead");
    }

    #[test]
    fn a_patch_whose_absence_adds_false_findings_guards_them() {
        let e = effect("x", &eval(4, 1), &eval(4, 3));
        assert_eq!(e.verdict(), "guards");
        assert_eq!(e.false_positives, 2);
        assert_eq!(e.profiles["de/png/news"], (0, 2));
    }

    #[test]
    fn a_patch_whose_absence_loses_a_value_guards_it() {
        let e = effect("x", &eval(4, 1), &eval(3, 1));
        assert_eq!((e.misses, e.verdict()), (1, "guards"));
    }

    #[test]
    fn the_inventory_of_the_repository_reads_and_names_known_patches() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../eval/patches.toml");
        let inv = Inventory::read(&path).unwrap();
        for p in &inv.patch {
            assert!(
                veilq_core::patch::PATCHES.contains(&p.id.as_str()) || p.id.starts_with("context:"),
                "{} is not in veilq_core::patch::PATCHES",
                p.id
            );
            for f in &p.corpus {
                assert!(
                    path.with_file_name(f).is_file(),
                    "{}: {f} is not in the corpus",
                    p.id
                );
            }
        }
        for id in veilq_core::patch::PATCHES {
            assert!(
                inv.patch.iter().any(|p| p.id == *id),
                "{id} is not listed in eval/patches.toml"
            );
        }
    }

    /// Every context boost of a locale's pattern is listed, and every one
    /// listed is a pattern with a context: a typo would measure nothing.
    #[test]
    fn the_inventory_lists_the_context_boosts_of_the_locales() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let inv = Inventory::read(&root.join("eval/patches.toml")).unwrap();
        let listed: std::collections::BTreeSet<&str> = inv
            .patch
            .iter()
            .filter_map(|p| p.id.strip_prefix("context:"))
            .collect();
        let mut boosted = std::collections::BTreeSet::new();
        for locale in ["de", "en", "intl"] {
            let path = root.join(format!("crates/veilq-core/locales/{locale}/patterns.toml"));
            let pack: toml::Value =
                toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            for p in pack["patterns"].as_array().unwrap() {
                if p.get("context").is_some() {
                    boosted.insert(format!("{locale}.{}", p["name"].as_str().unwrap()));
                }
            }
        }
        let boosted: std::collections::BTreeSet<&str> =
            boosted.iter().map(String::as_str).collect();
        assert_eq!(listed, boosted);
    }
}
