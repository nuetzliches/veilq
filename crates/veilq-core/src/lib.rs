//! The detection and redaction engine of veilq.
//!
//! The pipeline is `extract -> detect -> decide -> apply -> verify`
//! (docs/plan.md §3). Nothing is built yet: the request/response schema, the
//! policy file and the placeholder format wait for the review card of M0.
