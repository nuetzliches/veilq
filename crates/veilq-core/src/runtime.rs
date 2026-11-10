//! ONNX Runtime, loaded at run time from its pinned library (ADR-0022): one
//! environment per process, shared by NER and OCR.

use std::path::Path;
use std::sync::OnceLock;

use ort::environment::Environment;

use crate::Error;

/// The process's environment; the first call loads the library at `runtime`.
pub fn env(runtime: &Path) -> Result<&'static Environment, Error> {
    static ENV: OnceLock<Environment> = OnceLock::new();
    if let Some(env) = ENV.get() {
        return Ok(env);
    }
    let env = ort::init_from(runtime)
        .map_err(|e| Error::Format(format!("ONNX Runtime {}: {e}", runtime.display())))?
        .with_name("veilq")
        .build()
        .map_err(|e| Error::Format(format!("ONNX Runtime: {e}")))?;
    Ok(ENV.get_or_init(|| env))
}
