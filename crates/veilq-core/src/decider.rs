//! The learned decider (#35): a tree ensemble over a candidate's named
//! features, after the policy has decided.
//!
//! Its power is narrow by the approved review card: it never drops what a
//! rule keeps, and it touches neither the allow and deny lists nor a
//! checksum. It may only **lift** a candidate below its threshold: lifted,
//! the candidate is a finding, and the report names how many were lifted
//! among its review reasons. Without a head, or switched off (`--no-decider`,
//! `[decider] enabled = false`), every decision is the policy's.
//!
//! A head is JSON: a manifest, the names of its features, and trees whose
//! leaves add up to a score (before a logistic function). Feature names are
//! `confidence`, `margin` (confidence minus threshold), `rule=<feature>`
//! ([`crate::detect::FEATURES`]), `entity=<TYPE>` and `detector=<id>`; a name
//! the candidate does not have is 0 when it has a `=`, missing otherwise (an
//! encoder's dimension, which comes with the encoder).

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
    /// lifted.
    lift_at: f32,
    trees: Vec<Tree>,
}

/// A loaded, checked head.
#[derive(Debug, Clone, PartialEq)]
pub struct Decider {
    head: HeadFile,
}

impl Decider {
    /// Reads a head; an error when it is not one this veilq can evaluate:
    /// another version, a feature or a child out of range, a node that
    /// points back (no tree may loop), a lift threshold outside (0, 1).
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let bad = |m: String| Error::Format(format!("decider head: {m}"));
        let head: HeadFile = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
        if head.version != HEAD_VERSION {
            return Err(bad(format!(
                "version {} (this veilq reads {HEAD_VERSION})",
                head.version
            )));
        }
        if !(head.lift_at > 0.0 && head.lift_at < 1.0) {
            return Err(bad(format!(
                "lift_at {} is not between 0 and 1",
                head.lift_at
            )));
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

    /// The score of a candidate, between 0 and 1: how sure the head is
    /// that it is personal data.
    pub fn score(&self, c: &Candidate, threshold: f32, rules: &[&str]) -> f32 {
        let x: Vec<f32> = self
            .head
            .features
            .iter()
            .map(|name| value(name, c, threshold, rules))
            .collect();
        let sum: f32 = self.head.trees.iter().map(|t| leaf(t, &x)).sum::<f32>() + self.head.base;
        1.0 / (1.0 + (-sum).exp())
    }

    /// Whether a candidate below its threshold is lifted into the findings.
    /// The caller asks only for those (the card's first point).
    pub fn lifts(&self, c: &Candidate, threshold: f32, rules: &[&str]) -> bool {
        self.score(c, threshold, rules) >= self.head.lift_at
    }
}

fn value(name: &str, c: &Candidate, threshold: f32, rules: &[&str]) -> f32 {
    match name {
        "confidence" => c.confidence,
        "margin" => c.confidence - threshold,
        _ => match name.split_once('=') {
            Some(("rule", r)) => one(rules.contains(&r)),
            Some(("entity", e)) => one(c.entity.as_str() == e),
            Some(("detector", d)) => one(c.detector == d),
            Some(_) => 0.0,
            None => f32::NAN,
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

    #[test]
    fn a_head_that_cannot_be_evaluated_is_an_error() {
        for (from, to) in [
            (r#""version": 1"#, r#""version": 2"#),
            (r#""lift_at": 0.5"#, r#""lift_at": 1.5"#),
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
