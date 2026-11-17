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
use veilq_core::{Area, Engine, EntityType, Format, Location};

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
    /// Personal data veilq left in an area of a picture or a PDF page.
    MissedArea {
        area: Area,
        entity: EntityType,
        #[serde(default)]
        label: Option<String>,
    },
    /// A box on a picture released: nothing in `area` is personal data.
    ExtraArea { area: Area, entity: EntityType },
}

impl Mark {
    /// The value of a mark on text; `None` for an area.
    fn value(&self) -> Option<&str> {
        match self {
            Mark::Missed { value, .. } | Mark::Extra { value, .. } => Some(value.trim()),
            Mark::MissedArea { .. } | Mark::ExtraArea { .. } => None,
        }
    }

    fn area(&self) -> Option<&Area> {
        match self {
            Mark::MissedArea { area, .. } | Mark::ExtraArea { area, .. } => Some(area),
            Mark::Missed { .. } | Mark::Extra { .. } => None,
        }
    }
}

/// `engine` with `marks` applied to its run.
pub fn apply(engine: &Engine, marks: &[Mark]) -> Result<Engine, Failure> {
    let mut missed = Vec::new();
    let mut released = Vec::new();
    let mut missed_areas = Vec::new();
    let mut released_areas = Vec::new();
    for mark in marks {
        if mark.value().is_some_and(str::is_empty)
            || mark.area().is_some_and(|a| a.width == 0 || a.height == 0)
        {
            return Err(Failure(StatusCode::BAD_REQUEST, "an empty mark".into()));
        }
        match mark {
            Mark::Missed { entity, value, .. } => {
                missed.push((entity.clone(), value.trim().to_owned()))
            }
            Mark::Extra { value, .. } => released.push(value.trim().to_owned()),
            Mark::MissedArea { area, entity, .. } => missed_areas.push((*area, entity.clone())),
            Mark::ExtraArea { area, .. } => released_areas.push(*area),
        }
    }
    let engine = engine
        .with_marks(&missed, &released)
        .map_err(Failure::from)?;
    Ok(if missed_areas.is_empty() && released_areas.is_empty() {
        engine
    } else {
        engine.with_picture_marks(missed_areas, released_areas)
    })
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
    /// Whether a synthetic twin reproduces the mistake and was kept.
    twin: bool,
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

/// Whether areas can be marked on `format`: a still image, a PDF's pages.
pub fn takes_areas(format: Format) -> bool {
    format.is_picture() && format != Format::Gif
}

/// The case of an area marked on a picture or a PDF page: where it is, and
/// the picture around it as the excerpt.
fn area_case(
    state: &AppState,
    request: &CaseRequest,
    format: Format,
    bytes: &[u8],
) -> Result<(Case, Vec<u8>), Failure> {
    let (kind, area, entity, label) = match &request.mark {
        Mark::MissedArea {
            area,
            entity,
            label,
        } => (Kind::Missed, area, entity, label.clone()),
        Mark::ExtraArea { area, entity } => (Kind::Extra, area, entity, None),
        _ => unreachable!("an area mark"),
    };
    if !takes_areas(format) {
        return Err(unprocessable(format!(
            "areas are marked on images and PDF pages, not on {}",
            format.name()
        )));
    }
    let picture = if format == Format::Pdf {
        let page = area
            .frame
            .filter(|p| *p >= 1)
            .ok_or_else(|| unprocessable("an area on a PDF names its page"))?;
        let (pages, _) = state
            .ready_engine()?
            .pdf_pages(bytes, page as usize)
            .map_err(Failure::from)?;
        pages
            .into_iter()
            .nth(page as usize - 1)
            .ok_or_else(|| unprocessable(format!("the PDF has no page {page}")))?
    } else {
        bytes.to_vec()
    };
    let excerpt = crop(&picture, area)?;
    let location = Location::Region {
        part: None,
        frame: area.frame,
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };
    let mut case = Case::new(
        kind,
        entity.clone(),
        format.name(),
        location,
        &state.policy().0,
    );
    case.label = label;
    Ok((case, excerpt))
}

/// The picture around `area`, as PNG.
#[cfg(feature = "ocr")]
fn crop(picture: &[u8], area: &Area) -> Result<Vec<u8>, Failure> {
    veilq_core::visual::crop(picture, area, AREA_MARGIN).map_err(|e| unprocessable(e.to_string()))
}

#[cfg(not(feature = "ocr"))]
fn crop(_: &[u8], _: &Area) -> Result<Vec<u8>, Failure> {
    Err(unprocessable("this build reads no pictures"))
}

/// Pixels of the picture kept around a marked area in its excerpt.
#[cfg_attr(not(feature = "ocr"), allow(dead_code))]
const AREA_MARGIN: u32 = 24;

fn add_case(state: &AppState, request: CaseRequest) -> Result<CaseAdded, Failure> {
    let (name, bytes) = source_bytes(state, request.source.clone())?;
    let format = match &name {
        Some(n) => Format::from_path(std::path::Path::new(n)).map_err(Failure::from)?,
        None => Format::Text,
    };
    if request.mark.area().is_some() {
        let (case, excerpt) = area_case(state, &request, format, &bytes)?;
        return keep(state, case, Some(("image/png", &excerpt)));
    }
    if format.is_container() {
        return Err(unprocessable(format!(
            "marks on the text of {} come later; text, CSV and JSON can be marked, and areas on images and PDF pages",
            format.name()
        )));
    }
    let input = String::from_utf8(bytes).map_err(|_| unprocessable("not UTF-8 text"))?;
    let value = request.mark.value().unwrap_or_default().to_owned();
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
        Mark::MissedArea { .. } | Mark::ExtraArea { .. } => unreachable!("handled above"),
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
    let mut added = keep(
        state,
        case,
        reading.map(|r| ("text/plain", r.text.as_bytes())),
    )?;
    if let Some((text, span)) = reading.and_then(|r| twin(&base, &request.mark, &r.text)) {
        store(state)?
            .set_twin(&added.id, &text, span)
            .map_err(Failure::from)?;
        added.twin = true;
    }
    Ok(added)
}

/// The synthetic twin of a miss marked on text (#34, card point 6):
/// `excerpt` with the values replaced by stand-ins (`fake`), the marked one
/// too, and where the marked value's stand-in is in it. `None` unless veilq,
/// without the mark, leaves that stand-in as it left the value, and none of
/// the original values veilq replaced or the user marked is in it.
///
/// A box released has no twin: it would have to keep the released value to
/// make the same mistake, and a twin holds no original value; were the
/// release wrong, a value would lie on the disk in the clear.
fn twin(base: &Engine, mark: &Mark, excerpt: &str) -> Option<(String, std::ops::Range<usize>)> {
    let Mark::Missed { value, .. } = mark else {
        return None;
    };
    let value = value.trim();
    let out = apply(base, std::slice::from_ref(mark))
        .ok()?
        .with_action(Some(veilq_core::Action::Fake))
        .ok()?
        .redact_str(excerpt, Format::Text)
        .ok()?;
    let originals = out
        .input_spans
        .iter()
        .filter_map(|s| excerpt.get(s.clone()))
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if originals.chain([value]).any(|v| out.text.contains(v)) {
        return None;
    }
    let at = excerpt.find(value)?;
    let end = at + value.len();
    let i = out
        .input_spans
        .iter()
        .position(|s| s.start <= at && end <= s.end)?;
    let span = out.output_spans.get(i)?.clone();
    // The same mistake: veilq without the mark leaves the stand-in.
    let check = base.redact_str(&out.text, Format::Text).ok()?;
    let found = check
        .input_spans
        .iter()
        .any(|s| s.start < span.end && span.start < s.end);
    (!found).then_some((out.text, span))
}

/// Keeps `case` with its excerpt, encrypted to the key store's key.
fn keep(
    state: &AppState,
    case: Case,
    excerpt: Option<(&str, &[u8])>,
) -> Result<CaseAdded, Failure> {
    let key = (state.cases.key)();
    let stored = store(state)?
        .add(
            case,
            excerpt,
            key.as_ref().map(|k| k.to_public()).as_ref(),
            veilq_core::cases::now(),
        )
        .map_err(Failure::from)?;
    Ok(CaseAdded {
        excerpt: stored.excerpt.is_some(),
        id: stored.id,
        twin: false,
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

// ---- rules from marks (card point 5) -------------------------------------

/// A rule a mark proposes, the cheapest first: a value always found as its
/// type (a deny value, ADR-0044), or one never found (an allow value).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    Deny { entity: EntityType, value: String },
    Allow { value: String },
}

impl Rule {
    fn of(mark: &Mark) -> Option<Self> {
        match mark {
            Mark::Missed { value, entity, .. } => Some(Rule::Deny {
                entity: entity.clone(),
                value: value.trim().to_owned(),
            }),
            Mark::Extra { value, .. } => Some(Rule::Allow {
                value: value.trim().to_owned(),
            }),
            // An area holds no value a rule could name.
            Mark::MissedArea { .. } | Mark::ExtraArea { .. } => None,
        }
    }

    fn mark(&self) -> Mark {
        match self {
            Rule::Deny { entity, value } => Mark::Missed {
                value: value.clone(),
                entity: entity.clone(),
                label: None,
            },
            Rule::Allow { value } => Mark::Extra {
                value: value.clone(),
                entity: EntityType::new("PERSON").expect("a valid type"),
            },
        }
    }

    /// The rule in a policy: a `[[deny]]` table of its own (exact value,
    /// no parts), or the value added to `[allow] values`.
    pub(super) fn add_to(&self, policy: &mut veilq_core::Policy) {
        match self {
            Rule::Deny { entity, value } => policy.deny.push(veilq_core::policy::Deny {
                entity: entity.clone(),
                values: vec![value.clone()],
                files: Vec::new(),
                parts: false,
                read: None,
            }),
            Rule::Allow { value } => {
                if !policy.allow.values.contains(value) {
                    policy.allow.values.push(value.clone());
                }
            }
        }
    }

    /// The rule as TOML, to copy into a project's `veilq.toml`.
    pub(super) fn toml(&self) -> String {
        let quoted = |v: &str| serde_json::to_string(v).expect("a string serialises");
        match self {
            Rule::Deny { entity, value } => format!(
                "[[deny]]\nentity = {}\nvalues = [{}]\nparts = false\n",
                quoted(entity.as_str()),
                quoted(value)
            ),
            Rule::Allow { value } => format!("[allow]\nvalues = [{}]\n", quoted(value)),
        }
    }
}

/// What a rule changes on a text: findings it adds and findings it drops.
#[derive(Debug, Default, Serialize)]
pub struct Change {
    gained: usize,
    lost: usize,
}

impl Change {
    fn add(&mut self, other: Change) {
        self.gained += other.gained;
        self.lost += other.lost;
    }
}

/// The findings of `text` without and with `rule`, compared by place.
fn change(base: &Engine, rule: &Rule, text: &str, format: Format) -> Result<Change, Failure> {
    let without = base.redact_str(text, format).map_err(Failure::from)?;
    let with = apply(base, &[rule.mark()])?
        .redact_str(text, format)
        .map_err(Failure::from)?;
    let has = |r: &veilq_core::Redacted, l: &veilq_core::Location| {
        r.report.findings.iter().any(|f| &f.location == l)
    };
    Ok(Change {
        gained: with
            .report
            .findings
            .iter()
            .filter(|f| !has(&without, &f.location))
            .count(),
        lost: without
            .report
            .findings
            .iter()
            .filter(|f| !has(&with, &f.location))
            .count(),
    })
}

#[derive(Serialize)]
pub struct Proposal {
    rule: Rule,
    /// On this document.
    document: Change,
    /// On the earlier cases: their excerpts where the key reads them, else
    /// their twins; how many were tried.
    cases: Change,
    tried: usize,
    /// The rule as TOML, to copy where a project's policy is in effect.
    toml: String,
    /// Whether the user's policy is the one in use: then the rule is saved
    /// there, else it is offered to copy.
    user_in_use: bool,
}

/// `POST /api/cases/propose`: the rule a mark proposes, tried on the
/// document and on every earlier case.
pub async fn propose(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<CaseRequest>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || propose_rule(&state, request)).await;
    match result {
        Ok(Ok(p)) => axum::Json(p).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn propose_rule(state: &AppState, request: CaseRequest) -> Result<Proposal, Failure> {
    let rule = Rule::of(&request.mark)
        .ok_or_else(|| unprocessable("an area holds no value a rule could name yet"))?;
    let (name, bytes) = source_bytes(state, request.source)?;
    let format = match &name {
        Some(n) => Format::from_path(std::path::Path::new(n)).map_err(Failure::from)?,
        None => Format::Text,
    };
    if format.is_container() {
        return Err(unprocessable(format!(
            "rules are tried on text for now, not on {}",
            format.name()
        )));
    }
    let input = String::from_utf8(bytes).map_err(|_| unprocessable("not UTF-8 text"))?;
    let base = state
        .ready_engine()?
        .with_min_confidence(request.min_confidence)
        .map_err(Failure::from)?;
    let document = change(&base, &rule, &input, format)?;
    let store = store(state)?;
    let key = (state.cases.key)();
    let mut cases = Change::default();
    let mut tried = 0;
    for case in store.list().map_err(Failure::from)? {
        let excerpt = match (&case.excerpt, &key) {
            (Some(e), Some(k)) if e.media_type == "text/plain" => store
                .excerpt(&case.id, k)
                .ok()
                .flatten()
                .and_then(|b| String::from_utf8(b).ok()),
            _ => None,
        };
        let Some(text) = excerpt.or_else(|| store.twin(&case.id).ok().flatten()) else {
            continue;
        };
        cases.add(change(&base, &rule, &text, Format::Text)?);
        tried += 1;
    }
    Ok(Proposal {
        toml: rule.toml(),
        rule,
        document,
        cases,
        tried,
        user_in_use: super::settings::is_user_in_use(state),
    })
}

/// `POST /api/policy/rule`: a rule saved into the user's policy (ADR-0028),
/// checked as the settings save it. Refused where a project's policy is in
/// effect: there the rule is copied into it by hand.
pub async fn adopt(
    State(state): State<Arc<AppState>>,
    axum::Json(rule): axum::Json<Rule>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        if !super::settings::is_user_in_use(&state) {
            return Err((
                StatusCode::CONFLICT,
                "a project's veilq.toml is in effect here: copy the rule into it".to_owned(),
            ));
        }
        let mut policy =
            super::settings::user_policy().map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;
        rule.add_to(&mut policy);
        super::settings::save_user_policy(&state, &super::settings::PolicyTry::of(policy))
    })
    .await;
    match result {
        Ok(Ok(saved)) => axum::Json(saved).into_response(),
        Ok(Err((status, e))) => api_error(status, e),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
