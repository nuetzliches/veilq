//! `detect` and `decide` (docs/plan.md §3.1): pattern rules from the
//! built-in locale packs and the policy find candidates; the policy's allow
//! lists, thresholds and the overlap rule decide which are findings.
//!
//! A locale pack is a policy's `[[patterns]]` in a file of its own,
//! `locales/<locale>/patterns.toml`, compiled into the binary (ADR-0010,
//! ADR-0011). Entity types are the same in every locale (`TAX_ID`); the
//! detector names the locale (`pattern:de.tax-id`).

pub mod validate;

use regex::Regex;
use serde::Deserialize;

use crate::finding::EntityType;
use crate::policy::{Context, PatternRule, Policy};
use crate::{Error, pseudonym};

/// The locales a policy can name.
pub const LOCALES: &[&str] = &["de", "en"];

/// The packs compiled into the binary; `intl` always runs.
const PACKS: &[(&str, &str)] = &[
    ("intl", include_str!("../../locales/intl/patterns.toml")),
    ("de", include_str!("../../locales/de/patterns.toml")),
    ("en", include_str!("../../locales/en/patterns.toml")),
];

/// What a failed `validate` costs (ADR-0012): a match without context falls
/// below the default threshold, one with context stays above it.
pub const VALIDATION_PENALTY: f32 = 0.3;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    patterns: Vec<PatternRule>,
}

/// The rules of the pack `locale`, as written.
pub fn pack(locale: &str) -> Result<Vec<PatternRule>, String> {
    let (_, text) = PACKS
        .iter()
        .find(|(l, _)| *l == locale)
        .ok_or_else(|| format!("no pack for locale `{locale}`"))?;
    let file: PackFile = toml::from_str(text).map_err(|e| format!("pack {locale}: {e}"))?;
    for rule in &file.patterns {
        rule.check(&format!("pack {locale}, pattern"))?;
    }
    Ok(file.patterns)
}

/// A span that may be personal data, before the policy decides.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Byte offsets into the text it was found in.
    pub start: usize,
    pub end: usize,
    pub entity: EntityType,
    pub confidence: f32,
    pub detector: String,
}

struct Rule {
    detector: String,
    entity: EntityType,
    regex: Regex,
    confidence: f32,
    context: Option<Context>,
    validate: Option<String>,
    group: Option<usize>,
}

impl Rule {
    fn compile(rule: &PatternRule, detector: String) -> Result<Self, String> {
        let regex = Regex::new(&rule.regex).map_err(|e| format!("{detector}: {e}"))?;
        let group = regex.capture_names().position(|n| n == Some("pii"));
        let context = rule.context.clone().map(|mut c| {
            c.words = c.words.iter().map(|w| w.to_lowercase()).collect();
            c
        });
        Ok(Self {
            detector,
            entity: rule.entity.clone(),
            regex,
            confidence: rule.confidence,
            context,
            validate: rule.validate.clone(),
            group,
        })
    }
}

/// Allow lists, compiled.
struct Allow {
    values: Vec<String>,
    patterns: Vec<Regex>,
}

/// The rules of one run: the `intl` pack, the policy's locales, the policy's
/// own patterns, and how the policy decides.
pub struct Detector {
    rules: Vec<Rule>,
    allow: Allow,
    /// The NER model, when it is installed (ADR-0022).
    #[cfg(feature = "ner")]
    ner: Option<std::sync::Arc<crate::ner::Ner>>,
    policy: Policy,
}

impl Detector {
    pub fn new(policy: &Policy) -> Result<Self, Error> {
        let invalid = |message: String| Error::Policy {
            path: "veilq.toml".into(),
            message,
        };
        let mut rules = Vec::new();
        let locales = std::iter::once("intl").chain(policy.locales.iter().map(String::as_str));
        for locale in locales {
            for rule in pack(locale).map_err(invalid)? {
                let detector = format!("pattern:{locale}.{}", rule.name);
                rules.push(Rule::compile(&rule, detector).map_err(invalid)?);
            }
        }
        for rule in &policy.patterns {
            let detector = format!("policy:{}", rule.name);
            rules.push(Rule::compile(rule, detector).map_err(invalid)?);
        }
        let allow = Allow {
            values: policy
                .allow
                .values
                .iter()
                .map(|v| pseudonym::normalise_plain(v))
                .collect(),
            patterns: policy
                .allow
                .patterns
                .iter()
                .map(|p| Regex::new(&format!("^(?:{p})$")).map_err(|e| invalid(e.to_string())))
                .collect::<Result<_, _>>()?,
        };
        Ok(Self {
            rules,
            allow,
            #[cfg(feature = "ner")]
            ner: None,
            policy: policy.clone(),
        })
    }

    /// Runs the NER model beside the patterns.
    #[cfg(feature = "ner")]
    pub fn set_ner(&mut self, ner: std::sync::Arc<crate::ner::Ner>) {
        self.ner = Some(ner);
    }

    /// Whether the NER model runs.
    pub fn has_ner(&self) -> bool {
        #[cfg(feature = "ner")]
        return self.ner.is_some();
        #[cfg(not(feature = "ner"))]
        false
    }

    /// Every candidate in `text`. `context` is what stands before the text
    /// and is not part of it: a column's or a key's name. A failing model is
    /// an error (ADR-0003).
    pub fn candidates(&self, text: &str, context: &str) -> Result<Vec<Candidate>, Error> {
        let mut out = Vec::new();
        #[cfg(feature = "ner")]
        if let Some(ner) = &self.ner {
            out.extend(ner.candidates(text)?);
        }
        for rule in &self.rules {
            for caps in rule.regex.captures_iter(text) {
                let m = match rule.group {
                    Some(g) => match caps.get(g) {
                        Some(m) => m,
                        None => continue,
                    },
                    None => caps.get(0).expect("group 0 always matches"),
                };
                if m.is_empty() {
                    continue;
                }
                let mut confidence = rule.confidence;
                if let Some(c) = &rule.context
                    && has_context(c, context, &text[..m.start()])
                {
                    confidence += c.boost;
                }
                if let Some(v) = &rule.validate
                    && !validate::check(v, m.as_str())
                {
                    confidence -= VALIDATION_PENALTY;
                }
                out.push(Candidate {
                    start: m.start(),
                    end: m.end(),
                    entity: rule.entity.clone(),
                    confidence: round(confidence.clamp(0.0, 1.0)),
                    detector: rule.detector.clone(),
                });
            }
        }
        Ok(out)
    }

    /// The findings in `text`: of overlapping candidates the longest, then
    /// the most confident; of those, the ones the allow lists do not name, at
    /// or above their type's threshold. Sorted by position.
    ///
    /// Overlaps are resolved before the threshold: a long candidate below it
    /// (an IBAN with a wrong checksum) still claims its span, so that a short
    /// rule does not find a phone number in its digits.
    pub fn findings(&self, text: &str, context: &str) -> Result<Vec<Candidate>, Error> {
        let mut kept = self.candidates(text, context)?;
        kept.sort_by(|a, b| {
            (b.end - b.start)
                .cmp(&(a.end - a.start))
                .then(b.confidence.total_cmp(&a.confidence))
                .then(a.start.cmp(&b.start))
        });
        let mut chosen: Vec<Candidate> = Vec::new();
        for c in kept {
            if chosen.iter().all(|k| c.end <= k.start || c.start >= k.end) {
                chosen.push(c);
            }
        }
        chosen.retain(|c| {
            c.confidence >= round(self.threshold(&c.entity)) && !self.allowed(&text[c.start..c.end])
        });
        chosen.sort_by_key(|c| c.start);
        Ok(chosen)
    }

    pub fn threshold(&self, entity: &EntityType) -> f32 {
        self.policy
            .entities
            .get(entity)
            .and_then(|r| r.min_confidence)
            .unwrap_or(self.policy.defaults.min_confidence)
    }

    fn allowed(&self, value: &str) -> bool {
        let n = pseudonym::normalise_plain(value);
        self.allow.values.contains(&n) || self.allow.patterns.iter().any(|p| p.is_match(value))
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }
}

/// Confidences in thousandths: `1.0 - 0.3` is `0.7`, not `0.69999998`.
fn round(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}

/// Whether one of `c.words` is in the last `c.window` characters before the
/// match: of `context` (a column's name) and the text before it.
fn has_context(c: &Context, context: &str, before: &str) -> bool {
    let joined = format!("{context}\n{before}").to_lowercase();
    let start = joined
        .char_indices()
        .rev()
        .nth(c.window.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    let window = &joined[start..];
    c.words.iter().any(|w| {
        let starts_word = w.chars().next().is_some_and(char::is_alphanumeric);
        window.match_indices(w.as_str()).any(|(i, _)| {
            !starts_word
                || window[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|p| !p.is_alphanumeric())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(text: &str, context: &str) -> Vec<(String, String)> {
        let d = Detector::new(&Policy::default()).unwrap();
        d.findings(text, context)
            .unwrap()
            .into_iter()
            .map(|c| {
                (
                    c.entity.as_str().to_owned(),
                    text[c.start..c.end].to_owned(),
                )
            })
            .collect()
    }

    fn found(text: &str, entity: &str, value: &str) -> bool {
        findings(text, "")
            .iter()
            .any(|(e, v)| e == entity && v == value)
    }

    #[test]
    fn every_pack_compiles() {
        for (locale, _) in PACKS {
            assert!(!pack(locale).unwrap().is_empty(), "{locale}");
        }
    }

    #[test]
    fn the_demo_letter() {
        let text = "Hallo Jonas Brandt,\n\nFrau Mira Albers (geb. 14.03.1987) bittet um Rückruf \
            unter 0421 5550173 oder mira.albers@example.com. Anschrift: Lindenweg 12, 28195 Bremen.\n\
            IBAN DE89 3704 0044 0532 0130 00, Kennzeichen HB-MA 412, Steuer-ID 57 381 942 656.\n\
            Login von IP 203.0.113.24; Aktenzeichen 3 O 128/26.\n\nViele Grüße\nLena Vogt\n";
        for (entity, value) in [
            ("PERSON", "Jonas Brandt"),
            ("PERSON", "Mira Albers"),
            ("DATE_OF_BIRTH", "14.03.1987"),
            ("PHONE_NUMBER", "0421 5550173"),
            ("EMAIL_ADDRESS", "mira.albers@example.com"),
            ("ADDRESS", "Lindenweg 12, 28195 Bremen"),
            ("IBAN_CODE", "DE89 3704 0044 0532 0130 00"),
            ("LICENSE_PLATE", "HB-MA 412"),
            ("TAX_ID", "57 381 942 656"),
            ("IP_ADDRESS", "203.0.113.24"),
            ("COURT_FILE_NUMBER", "3 O 128/26"),
            ("PERSON", "Lena Vogt"),
        ] {
            assert!(
                found(text, entity, value),
                "{entity} {value}: {:?}",
                findings(text, "")
            );
        }
    }

    #[test]
    fn names_after_keys_and_roles() {
        assert!(found(
            "body={\"name\":\"Ida Sommer\",\"x\":1}",
            "PERSON",
            "Ida Sommer"
        ));
        assert!(found(
            "Bruder von Mira Albers; Kfz",
            "PERSON",
            "Mira Albers"
        ));
        assert!(!found("name=Projekt", "PERSON", "Projekt"));
    }

    #[test]
    fn lookalikes_stay() {
        let text = "Rechnung 2026-0421-5550 vom 02.11.2026, Version 10.4.21.7, \
            Referenz DE89 3704 0044 0532 0130 01, Nummer 57381942065.";
        assert_eq!(findings(text, ""), vec![]);
    }

    #[test]
    fn a_failed_check_with_context_is_still_found() {
        // A mistyped IBAN next to its label (ADR-0012).
        assert!(found(
            "IBAN: DE89 3704 0044 0532 0130 01",
            "IBAN_CODE",
            "DE89 3704 0044 0532 0130 01"
        ));
        // A valid check digit without context is not enough for a tax ID.
        assert!(!found("Nummer 57381942656", "TAX_ID", "57381942656"));
        assert!(found("Steuer-ID 57381942656", "TAX_ID", "57381942656"));
    }

    #[test]
    fn a_column_name_is_context() {
        assert_eq!(findings("14.03.1987", ""), vec![]);
        assert_eq!(
            findings("14.03.1987", "geburtsdatum"),
            vec![("DATE_OF_BIRTH".into(), "14.03.1987".into())]
        );
        assert_eq!(
            findings("Mira Albers", "name"),
            vec![("PERSON".into(), "Mira Albers".into())]
        );
        assert_eq!(findings("Mira Albers", "produkt"), vec![]);
    }

    #[test]
    fn context_words_start_words() {
        let c = Context {
            words: vec!["ip".into()],
            boost: 0.3,
            window: 20,
        };
        assert!(has_context(&c, "", "Login von IP "));
        assert!(!has_context(&c, "", "Zip "));
        assert!(has_context(&c, "client_ip", ""));
        assert!(!has_context(&c, "ip", &"x".repeat(40)));
    }

    #[test]
    fn the_policy_adds_patterns_and_allows_values() {
        let policy = Policy::parse(
            r#"
version = 1
locales = ["de"]
[[patterns]]
name = "contract"
entity = "CONTRACT_NUMBER"
regex = '\b\d{4}-\d{4}\b'
confidence = 0.5
context = { words = ["vertrag"], boost = 0.4 }
[allow]
values = ["Mira Albers"]
"#,
        )
        .unwrap();
        let d = Detector::new(&policy).unwrap();
        let text = "Vertrag 2026-0815 mit Frau Mira Albers";
        let f = d.findings(text, "").unwrap();
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].detector, "policy:contract");
        assert_eq!(&text[f[0].start..f[0].end], "2026-0815");
    }
}
