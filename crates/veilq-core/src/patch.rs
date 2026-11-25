//! The hand-written patches of the detection, by name, so that a
//! measurement can switch one off (#161).
//!
//! A patch is a rule written for one case: a word list that drops a
//! candidate (`KIN_WORDS`), a cut of a wrapped table cell, a context boost of a
//! locale's pattern. Each is listed in `eval/patches.toml` with the case it
//! protects; `veilq-eval ablate` reads the corpus once per patch with it
//! off and shows what it is worth.
//!
//! Only a build with the feature `ablation` (which `veilq-eval` turns on)
//! can switch one off. Without it [`on`] is always true and the shipped
//! binary has no such switch.

/// The patches that can be switched off, as `eval/patches.toml` names them.
/// A context boost of a pattern is `context:<locale>.<name>` and is not
/// listed here: every pattern's boost can be switched off.
pub const PATCHES: &[&str] = &[
    "compound-tail",
    "ocr-checksum-penalty",
    "zero-rule-column-head",
    "role-words",
    "possessive-drop",
    "accounts",
    "person-nouns",
    "kin-words",
    "birth-date-beside-name",
    "particles",
    "ner-name-shape",
    "ner-saluted-kin",
    "given-name-stopwords",
    "email-host-tags",
    "names-learn-filter",
    "legal-forms",
    "names-cut-edge",
    "names-glued",
    "names-similar",
    "values-learnable",
    "values-skip-ip",
    "placeholder-drop",
    "verify-learns-no-names",
    "table-cell-cut",
    "column-head-context",
    "formatting-marks",
    "iban-rest-widen",
    "weak-neighbours",
    "line-ends-in-name-pair",
    "address-block-pair",
    "office-person-metadata",
    "signature-tile-alone",
    "signature-min-width",
    "face-verify-gate",
];

/// Whether the patch `id` is on.
#[cfg(not(feature = "ablation"))]
#[inline(always)]
pub fn on(_id: &str) -> bool {
    true
}

/// Whether the context boost of the pattern `detector`
/// (`pattern:intl.name-column`) is on: the patch `context:intl.name-column`.
#[cfg(not(feature = "ablation"))]
#[inline(always)]
pub fn context_on(_detector: &str) -> bool {
    true
}

#[cfg(all(feature = "ablation", not(test)))]
static OFF: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

// The tests run side by side in one process: a test that switches a patch
// off switches it off for its own thread only.
#[cfg(all(feature = "ablation", test))]
thread_local! {
    static OFF: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Whether any patch switched off is `matches`.
#[cfg(feature = "ablation")]
fn off(matches: impl Fn(&str) -> bool) -> bool {
    #[cfg(not(test))]
    return OFF
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|o| matches(o));
    #[cfg(test)]
    OFF.with(|o| o.borrow().iter().any(|o| matches(o)))
}

/// Whether the patch `id` is on.
#[cfg(feature = "ablation")]
pub fn on(id: &str) -> bool {
    !off(|o| o == id)
}

/// Whether the context boost of the pattern `detector`
/// (`pattern:intl.name-column`) is on: the patch `context:intl.name-column`.
#[cfg(feature = "ablation")]
pub fn context_on(detector: &str) -> bool {
    let name = detector.strip_prefix("pattern:").unwrap_or(detector);
    !off(|o| o.strip_prefix("context:") == Some(name))
}

/// Switches the patches `ids` off for the whole process, the others on.
/// An id that names no patch is an error: a measurement of a typo would
/// measure nothing.
#[cfg(feature = "ablation")]
pub fn switch_off(ids: &[String]) -> Result<(), crate::Error> {
    for id in ids {
        if !PATCHES.contains(&id.as_str()) && !id.starts_with("context:") {
            return Err(crate::Error::Format(format!("no patch is called {id:?}")));
        }
    }
    #[cfg(not(test))]
    {
        *OFF.write().unwrap_or_else(|e| e.into_inner()) = ids.to_vec();
    }
    #[cfg(test)]
    OFF.with(|o| *o.borrow_mut() = ids.to_vec());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_patch_has_a_name_of_its_own() {
        let mut seen = std::collections::HashSet::new();
        for id in PATCHES {
            assert!(seen.insert(id), "{id} twice");
            assert!(
                !id.is_empty()
                    && !id.starts_with('-')
                    && !id.ends_with('-')
                    && !id.contains("--")
                    && id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{id} is not kebab-case"
            );
        }
    }
}
