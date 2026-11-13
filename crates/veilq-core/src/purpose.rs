//! Purposes (ADR-0040): what veilq is used for, as packs compiled into the
//! binary in the policy's format, like the locale packs (ADR-0010).
//!
//! A policy names its purposes (`purposes = ["legal"]`); `general` is always
//! one of them. A pack holds per type a threshold and an action, patterns,
//! values and patterns that are no personal data, and what a miss and a
//! false positive cost. Between packs the stricter rule wins; the policy's
//! own settings come before all of them. A pack only tightens: its
//! thresholds stay at or below the default, its actions black a value out;
//! it loosens only through `allow` and its costs. The costs are data for a
//! learned decider (#35); no threshold follows from them yet.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Action;
use crate::finding::EntityType;
use crate::policy::{Allow, EntityRule, LowConfidence, PatternRule};

/// The purposes a policy can name; `general` is in every policy.
pub const PURPOSES: &[&str] = &["general", "legal", "development"];
pub const GENERAL: &str = "general";

const PACKS: &[(&str, &str)] = &[
    ("general", include_str!("../purposes/general.toml")),
    ("legal", include_str!("../purposes/legal.toml")),
    ("development", include_str!("../purposes/development.toml")),
];

/// What a mistake on a type costs: missing a value, and blacking out what
/// is none. Their ratio is what a decider weighs (#35).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cost {
    pub miss: f32,
    pub false_positive: f32,
}

/// The cost of a type no purpose and no policy names: a miss ten times a
/// false positive.
pub const DEFAULT_COST: Cost = Cost {
    miss: 10.0,
    false_positive: 1.0,
};

impl Cost {
    /// The stricter of two: the higher cost of a miss, the lower of a false
    /// positive.
    fn stricter(self, other: Cost) -> Cost {
        Cost {
            miss: self.miss.max(other.miss),
            false_positive: self.false_positive.min(other.false_positive),
        }
    }
}

/// A purpose pack, as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purpose {
    /// One sentence: what the purpose is for.
    pub about: String,
    #[serde(default)]
    pub entities: BTreeMap<EntityType, EntityRule>,
    #[serde(default)]
    pub patterns: Vec<PatternRule>,
    #[serde(default)]
    pub allow: Allow,
    #[serde(default)]
    pub costs: BTreeMap<EntityType, Cost>,
    #[serde(default)]
    pub picture: PurposePicture,
}

/// What a purpose sets for pictures.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PurposePicture {
    /// Text OCR reads badly (ADR-0045); any value tightens `review`.
    #[serde(default)]
    pub low_confidence: Option<LowConfidence>,
}

/// The pack of the purpose `name`, checked: its patterns compile, and it
/// only tightens.
pub fn purpose(name: &str) -> Result<Purpose, String> {
    let (_, text) = PACKS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("unknown purpose `{name}` (known: {})", PURPOSES.join(", ")))?;
    let p: Purpose = toml::from_str(text).map_err(|e| format!("purpose {name}: {e}"))?;
    for rule in &p.patterns {
        rule.check(&format!("purpose {name}, pattern"))?;
    }
    for (entity, rule) in &p.entities {
        if let Some(c) = rule.min_confidence
            && !(0.0..=crate::policy::DEFAULT_MIN_CONFIDENCE).contains(&c)
        {
            return Err(format!(
                "purpose {name}: `entities.{}.min_confidence` {c} would loosen; a purpose only lowers a threshold",
                entity.as_str()
            ));
        }
        if let Some(a) = rule.action
            && !matches!(a, Action::Mask | Action::Remove)
        {
            return Err(format!(
                "purpose {name}: `entities.{}.action` must be `mask` or `remove`",
                entity.as_str()
            ));
        }
    }
    for (entity, c) in &p.costs {
        if !(c.miss > 0.0 && c.false_positive > 0.0) {
            return Err(format!(
                "purpose {name}: `costs.{}` must be above zero",
                entity.as_str()
            ));
        }
    }
    Ok(p)
}

/// The purposes in effect for `named`: `general` first, then the others in
/// their order, each once.
pub fn effective(named: &[String]) -> Vec<String> {
    let mut out = vec![GENERAL.to_owned()];
    for n in named {
        if !out.contains(n) {
            out.push(n.clone());
        }
    }
    out
}

/// How far an action blacks a value out, for the stricter of two.
fn strength(a: Action) -> u8 {
    match a {
        Action::Remove => 3,
        Action::Mask => 2,
        Action::Pseudonymize => 1,
        Action::Fake => 0,
    }
}

/// What the purposes `names` set, merged: per type the lower threshold and
/// the stronger action, the patterns of each (with its name), the allow
/// lists joined, per type the stricter cost.
pub struct Merged {
    pub entities: BTreeMap<EntityType, EntityRule>,
    pub patterns: Vec<(String, PatternRule)>,
    pub allow: Allow,
    pub costs: BTreeMap<EntityType, Cost>,
    /// The strictest of the purposes' (`error` over `box` over `review`).
    pub low_confidence: Option<LowConfidence>,
}

pub fn merge(names: &[String]) -> Result<Merged, String> {
    let mut m = Merged {
        entities: BTreeMap::new(),
        patterns: Vec::new(),
        allow: Allow::default(),
        costs: BTreeMap::new(),
        low_confidence: None,
    };
    for name in names {
        let p = purpose(name)?;
        for (entity, rule) in p.entities {
            let e = m.entities.entry(entity).or_default();
            e.min_confidence = match (e.min_confidence, rule.min_confidence) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            e.action = match (e.action, rule.action) {
                (Some(a), Some(b)) => Some(if strength(b) > strength(a) { b } else { a }),
                (a, b) => a.or(b),
            };
        }
        m.patterns
            .extend(p.patterns.into_iter().map(|r| (name.clone(), r)));
        for v in p.allow.values {
            if !m.allow.values.contains(&v) {
                m.allow.values.push(v);
            }
        }
        for v in p.allow.patterns {
            if !m.allow.patterns.contains(&v) {
                m.allow.patterns.push(v);
            }
        }
        for (entity, c) in p.costs {
            m.costs
                .entry(entity)
                .and_modify(|old| *old = old.stricter(c))
                .or_insert(c);
        }
        m.low_confidence = m.low_confidence.max(p.picture.low_confidence);
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_purpose_loads_and_only_tightens() {
        for name in PURPOSES {
            let p = purpose(name).unwrap_or_else(|e| panic!("{e}"));
            assert!(!p.about.trim().is_empty(), "{name}");
        }
        assert_eq!(PACKS.len(), PURPOSES.len());
        assert!(purpose("medical").is_err());
    }

    #[test]
    fn general_comes_first_and_once() {
        assert_eq!(effective(&[]), ["general"]);
        assert_eq!(
            effective(&["legal".into(), "general".into(), "legal".into()]),
            ["general", "legal"]
        );
    }

    #[test]
    fn between_purposes_the_stricter_rule_wins() {
        let m = merge(&effective(&["legal".into(), "development".into()])).unwrap();
        let person = EntityType::new("PERSON").unwrap();
        assert_eq!(m.entities[&person].min_confidence, Some(0.6));
        // Legal: a miss of a name costs 20; development: a false positive 3.
        assert_eq!(
            m.costs[&person],
            Cost {
                miss: 20.0,
                false_positive: 1.0
            }
        );
        assert!(
            m.patterns
                .iter()
                .any(|(n, r)| n == "development" && r.name == "github-token")
        );
    }

    fn policy(text: &str) -> crate::Policy {
        crate::Policy::parse(&format!("version = 1\n{text}")).unwrap()
    }

    fn found(policy: &crate::Policy, text: &str) -> Vec<(String, String)> {
        let engine = crate::Engine::new(policy, None, b"test", None).unwrap();
        let r = engine.scan_str(text, crate::Format::Text).unwrap();
        r.report
            .findings
            .iter()
            .map(|f| (f.entity.as_str().to_owned(), f.detector.clone()))
            .collect()
    }

    #[test]
    fn a_purpose_adds_its_patterns_and_the_report_names_it() {
        // Built at run time, so no secret scanner takes the test for a leak.
        let token = format!("gh{}_{}", "p", "a1B2".repeat(9));
        let text = format!("deploy failed, token {token}, key AKIAIOSFODNN7EXAMPLE\n");
        let general = crate::Policy::default();
        assert!(found(&general, &text).is_empty());
        let dev = policy("purposes = [\"development\"]");
        let f = found(&dev, &text);
        assert!(
            f.contains(&("SECRET".into(), "purpose:development.github-token".into())),
            "{f:?}"
        );
        assert!(
            f.contains(&("SECRET".into(), "purpose:development.aws-access-key".into())),
            "{f:?}"
        );
        let engine = crate::Engine::new(&dev, None, b"test", None).unwrap();
        let report = engine.scan_str(&text, crate::Format::Text).unwrap().report;
        assert_eq!(report.purposes, ["general", "development"]);
    }

    #[test]
    fn the_policy_comes_before_its_purposes() {
        let person = EntityType::new("PERSON").unwrap();
        let legal = policy("purposes = [\"legal\"]").resolved().unwrap();
        assert_eq!(legal.entities[&person].min_confidence, Some(0.6));
        assert_eq!(legal.cost(&person).miss, 20.0);
        // The policy's own threshold and cost, even where they loosen.
        let own = policy(
            "purposes = [\"legal\"]\n[entities.PERSON]\nmin_confidence = 0.9\n[costs.PERSON]\nmiss = 5.0\nfalse_positive = 1.0\n",
        )
        .resolved()
        .unwrap();
        assert_eq!(own.entities[&person].min_confidence, Some(0.9));
        assert_eq!(own.cost(&person).miss, 5.0);
        // A type nobody names costs the default.
        let iban = EntityType::new("IBAN_CODE").unwrap();
        assert_eq!(own.cost(&iban), DEFAULT_COST);
        // An unknown purpose is an error, not ignored.
        assert!(crate::Policy::parse("version = 1\npurposes = [\"medical\"]\n").is_err());
    }

    #[test]
    fn the_stronger_action_wins() {
        assert!(strength(Action::Remove) > strength(Action::Mask));
        assert!(strength(Action::Mask) > strength(Action::Pseudonymize));
        assert!(strength(Action::Pseudonymize) > strength(Action::Fake));
        let a = Cost {
            miss: 10.0,
            false_positive: 3.0,
        };
        let b = Cost {
            miss: 20.0,
            false_positive: 1.0,
        };
        assert_eq!(a.stricter(b), b);
    }
}
