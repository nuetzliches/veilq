//! Marks and cases in the UI (#34, review card points 1 to 3 and 7): a mark
//! acts on the document it was made on at once, and is kept as a case.
//!
//! A mark is a value the user selected in the output (`missed`: personal
//! data veilq left) or a box released (`extra`: none there). `/api/redact`
//! applies a document's marks to its run only ([`Engine::with_marks`]);
//! `/api/cases` keeps one as a case in the case store, with the line or
//! cell around it encrypted to the key in the OS key store. Text formats
//! for now; pictures come with their own marks.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use veilq_core::cases::{Case, CaseCandidate, Kind, Store};
use veilq_core::{Engine, EntityType, Format};

use super::{AppState, Failure, Source, api_error, source_bytes};

/// The key of the case store: a function, so that tests give their own.
pub type KeyFn = Arc<dyn Fn() -> Option<age::x25519::Identity> + Send + Sync>;

/// Where cases go and how their key is found.
pub struct Cases {
    pub dir: PathBuf,
    pub key: KeyFn,
}

// A test never touches the user's case store or key store.
#[cfg_attr(test, allow(dead_code))]
impl Cases {
    /// The user's case store and the OS key store's key.
    pub fn user() -> Self {
        Self {
            dir: veilq_core::cases::case_dir(),
            key: Arc::new(os_key),
        }
    }
}

#[cfg_attr(test, allow(dead_code))]
const SERVICE: &str = "veilq";
#[cfg_attr(test, allow(dead_code))]
const ACCOUNT: &str = "case-store";

/// The case store's key in the OS key store, made on first use; `None`
/// where there is no key store (the card: then no excerpt is kept).
#[cfg_attr(test, allow(dead_code))]
fn os_key() -> Option<age::x25519::Identity> {
    use age::secrecy::ExposeSecret;
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).ok()?;
    match entry.get_password() {
        Ok(text) => text.trim().parse().ok(),
        Err(keyring::Error::NoEntry) => {
            let key = age::x25519::Identity::generate();
            entry.set_password(key.to_string().expose_secret()).ok()?;
            Some(key)
        }
        Err(_) => None,
    }
}

/// A mark on one document.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mark {
    /// Personal data veilq left: `value` as it stands in the text.
    Missed {
        value: String,
        entity: EntityType,
        #[serde(default)]
        label: Option<String>,
    },
    /// A box released: `value` is no personal data.
    Extra { value: String, entity: EntityType },
}

impl Mark {
    fn value(&self) -> &str {
        match self {
            Mark::Missed { value, .. } | Mark::Extra { value, .. } => value,
        }
    }
}

/// `engine` with `marks` applied to its run.
pub fn apply(engine: &Engine, marks: &[Mark]) -> Result<Engine, Failure> {
    let mut missed = Vec::new();
    let mut released = Vec::new();
    for mark in marks {
        let value = mark.value().trim();
        if value.is_empty() {
            return Err(Failure(
                StatusCode::BAD_REQUEST,
                "a mark without a value".into(),
            ));
        }
        match mark {
            Mark::Missed { entity, .. } => missed.push((entity.clone(), value.to_owned())),
            Mark::Extra { .. } => released.push(value.to_owned()),
        }
    }
    engine.with_marks(&missed, &released).map_err(Failure::from)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRequest {
    source: Source,
    mark: Mark,
    /// The run's threshold, as `/api/redact` had it.
    #[serde(default)]
    min_confidence: Option<f32>,
}

#[derive(Serialize)]
pub struct CaseAdded {
    id: String,
    /// Whether the line or cell around the mark was kept (encrypted).
    excerpt: bool,
}

#[derive(Serialize)]
pub struct CaseCount {
    count: usize,
    /// Whether an excerpt can be kept: the OS has a key store.
    key_store: bool,
}

fn store(state: &AppState) -> Result<Store, Failure> {
    let store = Store::open(&state.cases.dir).map_err(Failure::from)?;
    store
        .purge(veilq_core::cases::now())
        .map_err(Failure::from)?;
    Ok(store)
}

/// `POST /api/cases`: keeps a mark as a case.
pub async fn add(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<CaseRequest>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || add_case(&state, request)).await;
    match result {
        Ok(Ok(added)) => axum::Json(added).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn unprocessable(message: impl Into<String>) -> Failure {
    Failure(StatusCode::UNPROCESSABLE_ENTITY, message.into())
}

fn add_case(state: &AppState, request: CaseRequest) -> Result<CaseAdded, Failure> {
    let (name, bytes) = source_bytes(state, request.source)?;
    let format = match &name {
        Some(n) => Format::from_path(std::path::Path::new(n)).map_err(Failure::from)?,
        None => Format::Text,
    };
    if format.is_container() {
        return Err(unprocessable(format!(
            "marks on {} come later; text, CSV and JSON can be marked",
            format.name()
        )));
    }
    let input = String::from_utf8(bytes).map_err(|_| unprocessable("not UTF-8 text"))?;
    let value = request.mark.value().trim().to_owned();
    let base = state
        .ready_engine()?
        .with_min_confidence(request.min_confidence)
        .map_err(Failure::from)?;
    let explanation = veilq_core::Explain::new();
    let before = base
        .with_explain(explanation.clone())
        .redact_str(&input, format)
        .map_err(Failure::from)?;
    let readings = explanation.take();
    // Where the mark is: a missed value where it is found once marked, a
    // released one where the run found it.
    let (kind, entity, label, location) = match &request.mark {
        Mark::Missed { entity, label, .. } => {
            let after = apply(&base, std::slice::from_ref(&request.mark))?
                .redact_str(&input, format)
                .map_err(Failure::from)?;
            let location = after
                .report
                .findings
                .iter()
                .find(|f| {
                    !before
                        .report
                        .findings
                        .iter()
                        .any(|b| b.location == f.location)
                })
                .map(|f| f.location.clone())
                .ok_or_else(|| unprocessable("the marked text is not in the input"))?;
            (Kind::Missed, entity.clone(), label.clone(), location)
        }
        Mark::Extra { entity, .. } => {
            let location = before
                .report
                .findings
                .iter()
                .zip(&before.input_spans)
                .find(|(_, span)| {
                    let span: &std::ops::Range<usize> = span;
                    input.get(span.clone()).map(str::trim) == Some(value.as_str())
                })
                .map(|(f, _)| f.location.clone())
                .ok_or_else(|| unprocessable("no box holds the released text"))?;
            (Kind::Extra, entity.clone(), None, location)
        }
    };
    // The excerpt: the line or cell veilq read it in, and what it proposed
    // there.
    let reading = readings
        .iter()
        .find(|r| !r.verify && r.text.contains(&value));
    let mut case = Case::new(kind, entity, format.name(), location, &state.policy().0);
    case.label = label;
    if let Some(r) = reading {
        let at = r.text.find(&value).unwrap_or(0);
        let end = at + value.len();
        case.candidates = r
            .candidates
            .iter()
            .filter(|c| c.start < end && at < c.end)
            .map(CaseCandidate::from)
            .collect();
    }
    let key = (state.cases.key)();
    let stored = store(state)?
        .add(
            case,
            reading.map(|r| ("text/plain", r.text.as_bytes())),
            key.as_ref().map(|k| k.to_public()).as_ref(),
            veilq_core::cases::now(),
        )
        .map_err(Failure::from)?;
    Ok(CaseAdded {
        excerpt: stored.excerpt.is_some(),
        id: stored.id,
    })
}

/// `GET /api/cases`: how many cases there are.
pub async fn count(State(state): State<Arc<AppState>>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<CaseCount, Failure> {
        Ok(CaseCount {
            count: store(&state)?.list().map_err(Failure::from)?.len(),
            key_store: (state.cases.key)().is_some(),
        })
    })
    .await;
    match result {
        Ok(Ok(c)) => axum::Json(c).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `DELETE /api/cases`: "Delete all cases".
pub async fn delete_all(State(state): State<Arc<AppState>>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<usize, Failure> {
        Store::open(&state.cases.dir)
            .and_then(|s| s.delete_all())
            .map_err(Failure::from)
    })
    .await;
    match result {
        Ok(Ok(n)) => axum::Json(serde_json::json!({ "deleted": n })).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
