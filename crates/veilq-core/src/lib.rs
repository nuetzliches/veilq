//! The detection and redaction engine of veilq.
//!
//! The pipeline is `extract -> detect -> decide -> apply -> verify`
//! (docs/plan.md §3). This crate holds the contracts the pipeline builds on
//! (ADR-0001 to ADR-0007): the pseudonym format, findings and reports, exit
//! outcomes, the policy file, the actions, the model requirement and the
//! root a caller's paths must stay under.

pub mod action;
pub mod detect;
pub mod engine;
pub mod error;
pub mod fake;
pub mod finding;
pub mod mail;
pub mod mapping;
pub mod marker;
pub mod models;
#[cfg(feature = "ner")]
pub mod ner;
#[cfg(feature = "ocr")]
pub mod ocr;
pub mod office;
pub mod outcome;
#[cfg(feature = "pdf")]
pub mod pdf;
pub mod policy;
pub mod pseudonym;
pub mod root;
#[cfg(feature = "models")]
pub mod runtime;
#[cfg(feature = "ocr")]
pub mod visual;
pub mod xml;

pub use action::Action;
pub use engine::{Engine, Explain, Explained, Format, Reading, Redacted};
pub use error::Error;
pub use finding::{EntityType, Finding, Location, Report};
pub use outcome::Outcome;
pub use policy::Policy;
pub use pseudonym::PseudonymKey;
