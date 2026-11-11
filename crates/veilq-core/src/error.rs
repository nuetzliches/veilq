use std::path::PathBuf;

/// Everything that stops a run. Each one ends it with [`crate::Outcome::Error`]
/// (exit code 2): veilq fails closed (ADR-0003).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("policy file {path}: {message}")]
    Policy { path: PathBuf, message: String },

    #[error("policy file {0} not found")]
    PolicyNotFound(PathBuf),

    #[error("the pseudonym key must be at least {min} bytes, got {got}")]
    KeyTooShort { min: usize, got: usize },

    #[error("action `{0}` is not built yet")]
    ActionNotBuilt(&'static str),

    /// A part of the pipeline that a later milestone builds.
    #[error("{0} is not built yet")]
    NotBuilt(&'static str),

    #[error("unsupported format {0}")]
    UnsupportedFormat(String),

    /// The input is not what its format says (broken JSON, an open quote).
    #[error("{0}")]
    Format(String),

    #[error("line {line} is not UTF-8 text")]
    NotUtf8 { line: u64 },

    /// verify found personal data in the output outside the replacements.
    /// The message names the type and detector, never the value.
    #[error("the output still contains {entity} (found by {detector}); nothing was written")]
    Leftover { entity: String, detector: String },

    /// In a picture, every verify round found something new to fill
    /// (ADR-0026). The message names the type and the place, never the
    /// value.
    #[error(
        "the picture still shows {entity} at {at} after {rounds} rounds of filling; nothing was written (`--unverified` writes it with that filled too, for a review)"
    )]
    Unsettled {
        entity: String,
        at: String,
        rounds: usize,
    },

    /// Writing or reading a mapping file (ADR-0018).
    #[error("mapping: {0}")]
    Mapping(String),

    #[error("reading or writing: {0}")]
    Stream(#[source] std::io::Error),

    #[error("action `pseudonymize` needs a key")]
    KeyMissing,

    /// `hint` says how to get the model, or that this release has none.
    #[error("the policy requires detector `{detector}`, but its model is not installed ({hint})")]
    ModelMissing {
        detector: String,
        hint: &'static str,
    },

    /// A download that is not the pinned file (ADR-0005).
    #[error(
        "{file} is not the pinned file (SHA-256 {got}, expected {expected}); nothing was installed"
    )]
    ModelMismatch {
        file: String,
        expected: String,
        got: String,
    },

    #[error("{path} is outside the allowed root {root}")]
    OutsideRoot { path: PathBuf, root: PathBuf },

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
