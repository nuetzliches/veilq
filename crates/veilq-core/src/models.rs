//! Detectors and the models they need (ADR-0005).
//!
//! A model is fetched only by `veilq models fetch`, from a URL with a SHA-256
//! compiled into the binary; a run never downloads. A detector the policy
//! requires whose model is not installed ends the run with exit code 2.
//! No model is pinned yet (plan O-1, O-2): `PINNED` stays empty until M3/M4.

use std::path::{Path, PathBuf};

use crate::Error;

/// The detectors a policy can name.
pub const DETECTORS: &[&str] = &["pattern", "ner", "ocr", "verifier"];

/// A model a detector needs, as compiled into the binary.
pub struct PinnedModel {
    pub detector: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

/// The models of this release.
pub const PINNED: &[PinnedModel] = &[];

/// Whether `detector` runs without a model (built into the binary, or an
/// endpoint the policy configures).
fn needs_no_model(detector: &str) -> bool {
    matches!(detector, "pattern" | "verifier")
}

/// Fails unless every detector in `required` can run with the models in
/// `model_dir`.
pub fn check_required(required: &[String], model_dir: &Path) -> Result<(), Error> {
    for detector in required {
        if needs_no_model(detector) {
            continue;
        }
        let pinned: Vec<&PinnedModel> = PINNED.iter().filter(|m| m.detector == detector).collect();
        let installed =
            !pinned.is_empty() && pinned.iter().all(|m| model_dir.join(m.file).is_file());
        if !installed {
            return Err(Error::ModelMissing {
                detector: detector.clone(),
                hint: if pinned.is_empty() {
                    "this release pins no model for it"
                } else {
                    "run `veilq models fetch`"
                },
            });
        }
    }
    Ok(())
}

/// Where models live: `VEILQ_MODEL_DIR`, else the user's cache directory.
pub fn model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VEILQ_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("veilq")
        .join("models")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_required_model_that_is_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_required(&["pattern".into(), "verifier".into()], dir.path()).is_ok());
        assert!(matches!(
            check_required(&["ner".into()], dir.path()),
            Err(Error::ModelMissing { detector, .. }) if detector == "ner"
        ));
    }
}
