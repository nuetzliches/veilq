//! The learned decider (#35): a tree ensemble over a candidate's named
//! features, after the policy has decided.
//!
//! Its power is narrow by the approved review cards: it never drops what a
//! rule keeps, and it touches neither the allow and deny lists nor a
//! checksum. A head of one kind may only **lift** a candidate below its
//! threshold: lifted, the candidate is a finding, and the report names how
//! many were lifted among its review reasons. A head of the other kind only
//! **marks** a weak finding in a picture as likely no personal data (#175,
//! ADR-0077): it stays filled, and the report says so per finding and among
//! its review reasons. Without a head, or switched off (`--no-decider`,
//! `[decider] enabled = false`, `[decider] demote = false`), every decision
//! is the policy's.
//!
//! A head is JSON: a manifest, the names of its features, and trees whose
//! leaves add up to a score (before a logistic function), with either
//! `lift_at` or `mark_below`. Feature names are `confidence`, `margin`
//! (confidence minus threshold), `rule=<feature>` ([`crate::detect::FEATURES`]),
//! `entity=<TYPE>`, `detector=<id>` and a candidate's named measures
//! ([`Measures`]); a name the candidate does not have is 0 when it has a
//! `=`, missing otherwise.

use serde::Deserialize;

use crate::Error;
use crate::detect::Candidate;

/// The head format's version.
pub const HEAD_VERSION: u32 = 1;

/// Said in the report's review reasons when the decider lifted candidates.
pub fn lifted_reason(n: usize) -> String {
    format!(
        "the learned decider lifted {n} candidate{} below {} threshold into the findings: check {}",
        if n == 1 { "" } else { "s" },
        if n == 1 { "its" } else { "their" },
        if n == 1 { "it" } else { "them" },
    )
}

/// Said in the report's review reasons when the decider marked findings
/// as likely no personal data (#175).
pub fn marked_reason(n: usize) -> String {
    format!(
        "the learned decider marked {n} finding{} as likely no personal data; {} still filled: check {}",
        if n == 1 { "" } else { "s" },
        if n == 1 { "it is" } else { "they are" },
        if n == 1 { "it" } else { "them" },
    )
}

/// Said in the report's review reasons when findings were dropped (#175
/// step 5): they are not filled.
pub fn dropped_reason(n: usize) -> String {
    format!(
        "the learned decider left {n} finding{} unfilled as likely no personal data, by the policy's `[decider] drop`: check {} under `dropped`",
        if n == 1 { "" } else { "s" },
        if n == 1 { "it" } else { "them" },
    )
}

/// Whether a review reason is [`marked_reason`]'s or [`dropped_reason`]'s:
/// a container counts them over its parts once.
pub(crate) fn is_doubt_reason(reason: &str) -> bool {
    reason.starts_with("the learned decider marked ")
        || reason.starts_with("the learned decider left ")
}

/// The head veilq brings that marks weak findings in pictures (#175):
/// trained in veilq-train on synthetic pictures of veilq-eval's corpus
/// only, measured on held-out seeds (ADR-0077).
pub const DEMOTE_HEAD: &str = include_str!("../assets/demote-v0.json");

/// What a head is and how it was made.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    /// The encoder whose vector the head reads, `None` for named features
    /// only.
    #[serde(default)]
    pub encoder: Option<String>,
    /// What it was trained on (the synthetic corpus and its seeds).
    pub trained_on: String,
    /// The gate's scores it passed (`recall`, `precision`, ...).
    #[serde(default)]
    pub gate: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum Node {
    Split {
        feature: usize,
        threshold: f32,
        left: usize,
        right: usize,
        /// Where a missing value goes.
        #[serde(default)]
        missing_left: bool,
    },
    Leaf {
        value: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tree {
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeadFile {
    version: u32,
    manifest: Manifest,
    features: Vec<String>,
    /// Added to the trees' sum before the logistic function.
    base: f32,
    /// The score at or above which a candidate below its threshold is
    /// lifted: a head that lifts.
    #[serde(default)]
    lift_at: Option<f32>,
    /// The score below which a weak finding is marked as likely no
    /// personal data: a head that marks (#175).
    #[serde(default)]
    mark_below: Option<f32>,
    /// Per class (a weak detector, [`WEAK_DETECTORS`]), the stricter score
    /// below which a marked finding may be dropped where the policy names
    /// the class (`[decider] drop`, #175 step 5, ADR-0078). A class enters
    /// only by the owner's approval.
    #[serde(default)]
    drop_below: std::collections::BTreeMap<String, f32>,
    trees: Vec<Tree>,
}

/// What a head that marks says of a finding (#175), from least to most
/// doubt: of several readings of one value, the least counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Doubt {
    /// Not marked: as the policy decided.
    None,
    /// Likely no personal data; filled all the same.
    Marked,
    /// Below its class's drop cut, and the policy drops the class: not
    /// filled, listed in the report's `dropped`.
    Dropped,
}

/// A loaded, checked head.
#[derive(Debug, Clone, PartialEq)]
pub struct Decider {
    head: HeadFile,
}

impl Decider {
    /// Reads a head; an error when it is not one this veilq can evaluate:
    /// another version, a feature or a child out of range, a node that
    /// points back (no tree may loop), not exactly one of `lift_at` and
    /// `mark_below`, or one outside (0, 1); a `drop_below` without
    /// `mark_below`, for a class that is no weak detector, or above
    /// `mark_below`.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let bad = |m: String| Error::Format(format!("decider head: {m}"));
        let head: HeadFile = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
        if head.version != HEAD_VERSION {
            return Err(bad(format!(
                "version {} (this veilq reads {HEAD_VERSION})",
                head.version
            )));
        }
        let (name, at) = match (head.lift_at, head.mark_below) {
            (Some(at), None) => ("lift_at", at),
            (None, Some(at)) => ("mark_below", at),
            _ => return Err(bad("not exactly one of lift_at and mark_below".into())),
        };
        if !(at > 0.0 && at < 1.0) {
            return Err(bad(format!("{name} {at} is not between 0 and 1")));
        }
        for (class, below) in &head.drop_below {
            if !WEAK_DETECTORS.contains(&class.as_str()) {
                return Err(bad(format!(
                    "drop_below for {class}, which is no weak detector"
                )));
            }
            match head.mark_below {
                Some(mark) if *below > 0.0 && *below <= mark => {}
                _ => {
                    return Err(bad(format!(
                        "drop_below {below} for {class} is not between 0 and mark_below"
                    )));
                }
            }
        }
        if head.trees.is_empty() {
            return Err(bad("no trees".into()));
        }
        for (t, tree) in head.trees.iter().enumerate() {
            if tree.nodes.is_empty() {
                return Err(bad(format!("tree {t} has no nodes")));
            }
            for (i, node) in tree.nodes.iter().enumerate() {
                if let Node::Split {
                    feature,
                    left,
                    right,
                    threshold,
                    ..
                } = node
                {
                    let n = tree.nodes.len();
                    if *feature >= head.features.len()
                        || *left <= i
                        || *right <= i
                        || *left >= n
                        || *right >= n
                        || !threshold.is_finite()
                    {
                        return Err(bad(format!("tree {t}, node {i}: out of range")));
                    }
                }
            }
        }
        Ok(Self { head })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.head.manifest
    }

    /// Whether this head marks weak findings (`mark_below`) rather than
    /// lifting candidates.
    pub fn marks_findings(&self) -> bool {
        self.head.mark_below.is_some()
    }

    /// The score of a candidate, between 0 and 1: how sure the head is
    /// that it is personal data.
    pub fn score(&self, c: &Candidate, threshold: f32, rules: &[&str]) -> f32 {
        self.score_with(c, threshold, rules, &Measures::new())
    }

    /// [`score`](Self::score) with the candidate's named measures.
    pub fn score_with(
        &self,
        c: &Candidate,
        threshold: f32,
        rules: &[&str],
        measures: &Measures,
    ) -> f32 {
        let x: Vec<f32> = self
            .head
            .features
            .iter()
            .map(|name| value(name, c, threshold, rules, measures))
            .collect();
        let sum: f32 = self.head.trees.iter().map(|t| leaf(t, &x)).sum::<f32>() + self.head.base;
        1.0 / (1.0 + (-sum).exp())
    }

    /// Whether a candidate below its threshold is lifted into the findings.
    /// The caller asks only for those (the card's first point). A head that
    /// marks lifts nothing.
    pub fn lifts(&self, c: &Candidate, threshold: f32, rules: &[&str]) -> bool {
        self.head
            .lift_at
            .is_some_and(|at| self.score(c, threshold, rules) >= at)
    }

    /// Whether a weak finding ([`weak`]) is marked as likely no personal
    /// data. A head that lifts marks nothing.
    pub fn marks(&self, c: &Candidate, threshold: f32, measures: &Measures) -> bool {
        self.doubt(c, threshold, measures, &[]) != Doubt::None
    }

    /// What the head says of a finding: marked below `mark_below`, dropped
    /// below its class's `drop_below` where `drop` (the policy's classes)
    /// names the class. Only a weak finding is ever marked or dropped.
    pub fn doubt(
        &self,
        c: &Candidate,
        threshold: f32,
        measures: &Measures,
        drop: &[String],
    ) -> Doubt {
        let Some(mark) = self.head.mark_below.filter(|_| weak(c)) else {
            return Doubt::None;
        };
        let score = self.score_with(c, threshold, &[], measures);
        let dropped = drop.contains(&c.detector)
            && self
                .head
                .drop_below
                .get(&c.detector)
                .is_some_and(|below| score < *below);
        if dropped {
            Doubt::Dropped
        } else if score < mark {
            Doubt::Marked
        } else {
            Doubt::None
        }
    }

    /// The classes this head may drop: those it carries a drop cut for.
    pub fn drop_classes(&self) -> Vec<&str> {
        self.head.drop_below.keys().map(String::as_str).collect()
    }
}

/// A candidate's named measures: its text's shape
/// ([`crate::engine::text_measures`]), its box in a picture (`box.*`), and
/// how often its value is a finding in the same reading (`file.repeats`).
pub type Measures = std::collections::BTreeMap<&'static str, f32>;

/// The detectors whose findings a head may mark (#175, the card's first
/// point): a name the NER model read, or one the file's names repeat.
/// Patterns, the policy's lists and unread areas never are.
pub const WEAK_DETECTORS: [&str; 2] = ["ner", "names:file"];

/// Whether a finding is one a head may mark: a person's name from a weak
/// detector.
pub fn weak(c: &Candidate) -> bool {
    c.entity.as_str() == "PERSON" && WEAK_DETECTORS.contains(&c.detector.as_str())
}

fn value(name: &str, c: &Candidate, threshold: f32, rules: &[&str], measures: &Measures) -> f32 {
    match name {
        "confidence" => c.confidence,
        "margin" => c.confidence - threshold,
        _ => match name.split_once('=') {
            Some(("rule", r)) => one(rules.contains(&r)),
            Some(("entity", e)) => one(c.entity.as_str() == e),
            Some(("detector", d)) => one(c.detector == d),
            Some(_) => 0.0,
            None => measures.get(name).copied().unwrap_or(f32::NAN),
        },
    }
}

fn one(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

fn leaf(tree: &Tree, x: &[f32]) -> f32 {
    let mut i = 0;
    loop {
        match &tree.nodes[i] {
            Node::Leaf { value } => return *value,
            Node::Split {
                feature,
                threshold,
                left,
                right,
                missing_left,
            } => {
                let v = x[*feature];
                let go_left = if v.is_nan() {
                    *missing_left
                } else {
                    v <= *threshold
                };
                i = if go_left { *left } else { *right };
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::EntityType;

    /// A head that lifts a date below its threshold beside a name, and
    /// nothing else: one tree on `rule=beside_name`.
    pub(crate) const BESIDE_NAME: &str = r#"{
        "version": 1,
        "manifest": {"name": "test", "trained_on": "a test"},
        "features": ["rule=beside_name", "confidence"],
        "base": -4.0,
        "lift_at": 0.5,
        "trees": [{"nodes": [
            {"feature": 0, "threshold": 0.5, "left": 1, "right": 2},
            {"value": 0.0},
            {"value": 8.0}
        ]}]
    }"#;

    fn date(confidence: f32) -> Candidate {
        Candidate {
            start: 0,
            end: 10,
            entity: EntityType::new("DATE_OF_BIRTH").unwrap(),
            confidence,
            detector: "pattern:de.birth-date".into(),
        }
    }

    #[test]
    fn a_head_scores_by_its_trees() {
        let d = Decider::from_json(BESIDE_NAME).unwrap();
        assert!(d.lifts(&date(0.3), 0.7, &["beside_name"]));
        assert!(!d.lifts(&date(0.3), 0.7, &["can_be_birth_date"]));
        assert!(d.score(&date(0.3), 0.7, &[]) < 0.05);
        assert_eq!(d.manifest().name, "test");
    }

    #[test]
    fn a_missing_value_goes_where_the_split_says() {
        let json = BESIDE_NAME
            .replace(r#""rule=beside_name""#, r#""vector:0""#)
            .replace(
                r#""left": 1, "right": 2}"#,
                r#""left": 1, "right": 2, "missing_left": false}"#,
            );
        let d = Decider::from_json(&json).unwrap();
        // `vector:0` is missing without an encoder: right, to the 8.0 leaf.
        assert!(d.lifts(&date(0.3), 0.7, &[]));
    }

    /// A head that marks a weak finding of four letters or fewer: one tree
    /// on `text.len`.
    pub(crate) const SHORT_NAMES: &str = r#"{
        "version": 1,
        "manifest": {"name": "test", "trained_on": "a test"},
        "features": ["text.len"],
        "base": 4.0,
        "mark_below": 0.5,
        "trees": [{"nodes": [
            {"feature": 0, "threshold": 4.5, "left": 1, "right": 2},
            {"value": -8.0},
            {"value": 0.0}
        ]}]
    }"#;

    #[test]
    fn a_head_that_marks_marks_weak_findings_only_and_lifts_nothing() {
        let d = Decider::from_json(SHORT_NAMES).unwrap();
        assert!(d.marks_findings());
        let name = |detector: &str| Candidate {
            start: 0,
            end: 4,
            entity: EntityType::new("PERSON").unwrap(),
            confidence: 0.9,
            detector: detector.into(),
        };
        let short = Measures::from([("text.len", 4.0)]);
        let long = Measures::from([("text.len", 11.0)]);
        assert!(d.marks(&name("ner"), 0.7, &short));
        assert!(d.marks(&name("names:file"), 0.7, &short));
        assert!(!d.marks(&name("ner"), 0.7, &long));
        // A pattern's finding, or another type, is never marked.
        assert!(!d.marks(&name("pattern:de.salutation"), 0.7, &short));
        assert!(!d.marks(&date(0.9), 0.7, &short));
        assert!(!d.lifts(&name("ner"), 0.95, &[]));
        // A head that lifts marks nothing.
        let lifting = Decider::from_json(BESIDE_NAME).unwrap();
        assert!(!lifting.marks_findings());
        assert!(!lifting.marks(&name("ner"), 0.7, &short));
    }

    #[test]
    fn a_class_drops_below_its_cut_only_where_the_policy_names_it() {
        let d = Decider::from_json(&SHORT_NAMES.replace(
            r#""mark_below": 0.5,"#,
            r#""mark_below": 0.5, "drop_below": {"names:file": 0.4},"#,
        ))
        .unwrap();
        assert_eq!(d.drop_classes(), vec!["names:file"]);
        let name = |detector: &str| Candidate {
            start: 0,
            end: 4,
            entity: EntityType::new("PERSON").unwrap(),
            confidence: 0.9,
            detector: detector.into(),
        };
        let short = Measures::from([("text.len", 4.0)]);
        let drop = ["names:file".to_owned()];
        assert_eq!(
            d.doubt(&name("names:file"), 0.7, &short, &drop),
            Doubt::Dropped
        );
        assert_eq!(
            d.doubt(&name("names:file"), 0.7, &short, &[]),
            Doubt::Marked
        );
        // A class without a cut is marked only, even where the policy names it.
        let ner = ["ner".to_owned()];
        assert_eq!(d.doubt(&name("ner"), 0.7, &short, &ner), Doubt::Marked);
        let long = Measures::from([("text.len", 11.0)]);
        assert_eq!(d.doubt(&name("names:file"), 0.7, &long, &drop), Doubt::None);
        assert!(Doubt::None < Doubt::Marked && Doubt::Marked < Doubt::Dropped);
        // The head veilq brings drops no class yet.
        assert!(
            Decider::from_json(DEMOTE_HEAD)
                .unwrap()
                .drop_classes()
                .is_empty()
        );
    }

    #[test]
    fn a_drop_cut_that_cannot_be_used_is_an_error() {
        for drop in [
            r#"{"names:file": 0.6}"#,
            r#"{"pattern:de.iban": 0.1}"#,
            r#"{"names:file": 0.0}"#,
        ] {
            let json = SHORT_NAMES.replace(
                r#""mark_below": 0.5,"#,
                &format!(r#""mark_below": 0.5, "drop_below": {drop},"#),
            );
            assert!(Decider::from_json(&json).is_err(), "{drop} loaded");
        }
        let lifting = BESIDE_NAME.replace(
            r#""lift_at": 0.5,"#,
            r#""lift_at": 0.5, "drop_below": {"ner": 0.1},"#,
        );
        assert!(Decider::from_json(&lifting).is_err());
    }

    #[test]
    fn the_head_veilq_brings_marks_and_reads_what_veilq_measures() {
        let d = Decider::from_json(DEMOTE_HEAD).unwrap();
        assert!(d.marks_findings());
        for f in &d.head.features {
            let measured = crate::engine::text_measures("Lena Brandt").contains_key(f.as_str())
                || f.starts_with("box.")
                || matches!(
                    f.as_str(),
                    "confidence"
                        | "margin"
                        | "file.repeats"
                        | "detector=ner"
                        | "detector=names:file"
                );
            assert!(measured, "the head reads {f}, which veilq does not measure");
        }
    }

    #[test]
    fn a_head_that_cannot_be_evaluated_is_an_error() {
        for (from, to) in [
            (r#""version": 1"#, r#""version": 2"#),
            (r#""lift_at": 0.5"#, r#""lift_at": 1.5"#),
            (r#""lift_at": 0.5"#, r#""lift_at": 0.5, "mark_below": 0.5"#),
            (r#""lift_at": 0.5,"#, ""),
            (r#""feature": 0"#, r#""feature": 7"#),
            (r#""left": 1"#, r#""left": 0"#),
            (r#""right": 2"#, r#""right": 9"#),
            (r#""base""#, r#""bias""#),
        ] {
            let json = BESIDE_NAME.replace(from, to);
            assert!(Decider::from_json(&json).is_err(), "{from} -> {to} loaded");
        }
    }
}
