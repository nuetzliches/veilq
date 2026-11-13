//! The UI's rules, folders and folder runs (ADR-0028).
//!
//! - **Rules:** the user's policy as a form: read, checked and tried on a
//!   sample, saved to [`policy::user_file`]; when it is the policy in use,
//!   the engine is built again at once.
//! - **Folders:** browsing the machine's folders (names only) and choosing
//!   the root the UI reads under.
//! - **A folder run:** every file under a folder of the root, redacted into
//!   a new folder; what veilq cannot read is listed, never copied.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use veilq_core::policy::{self, Policy};
use veilq_core::{Action, Engine, Format, root};

use super::{AppState, api_error, display_path};

// ---- rules ----------------------------------------------------------------

#[derive(Serialize)]
pub struct PolicyView {
    /// The policy in use and where it came from.
    source: String,
    /// The user's policy file, and whether it is the one in use (no
    /// project's `veilq.toml` comes first).
    user_file: Option<String>,
    user_in_use: bool,
    /// What the form edits: the user's file, or the defaults.
    policy: Policy,
    /// What the form offers.
    entities: Vec<String>,
    locales: &'static [&'static str],
    actions: Vec<&'static str>,
    validators: &'static [&'static str],
    /// The purposes a policy can name, each with what it changes against
    /// `general` (ADR-0040).
    purposes: Vec<PurposeView>,
}

/// A purpose as the form shows it: what it is for and what it adds.
#[derive(Serialize)]
pub struct PurposeView {
    name: &'static str,
    about: String,
    /// Per type the threshold and action it sets.
    entities: std::collections::BTreeMap<String, veilq_core::policy::EntityRule>,
    /// Its patterns: name and type.
    patterns: Vec<(String, String)>,
    /// Values and patterns it takes for no personal data.
    allow: usize,
    /// Per type what a miss and a false positive cost.
    costs: std::collections::BTreeMap<String, veilq_core::purpose::Cost>,
}

fn purposes() -> Vec<PurposeView> {
    veilq_core::purpose::PURPOSES
        .iter()
        .filter_map(|&name| {
            let p = veilq_core::purpose::purpose(name).ok()?;
            Some(PurposeView {
                name,
                about: p.about,
                entities: p
                    .entities
                    .into_iter()
                    .map(|(e, r)| (e.as_str().to_owned(), r))
                    .collect(),
                patterns: p
                    .patterns
                    .into_iter()
                    .map(|r| (r.name, r.entity.as_str().to_owned()))
                    .collect(),
                allow: p.allow.values.len() + p.allow.patterns.len(),
                costs: p
                    .costs
                    .into_iter()
                    .map(|(e, c)| (e.as_str().to_owned(), c))
                    .collect(),
            })
        })
        .collect()
}

/// The entity types the built-in detectors find.
fn entities() -> Vec<String> {
    let mut out: Vec<String> = std::iter::once("intl")
        .chain(veilq_core::detect::LOCALES.iter().copied())
        .flat_map(|l| veilq_core::detect::pack(l).unwrap_or_default())
        .map(|r| r.entity.as_str().to_owned())
        .chain(veilq_core::purpose::PURPOSES.iter().flat_map(|p| {
            veilq_core::purpose::purpose(p)
                .map(|p| p.patterns)
                .unwrap_or_default()
                .into_iter()
                .map(|r| r.entity.as_str().to_owned())
        }))
        .chain(["PERSON".to_owned()])
        .collect();
    out.sort();
    out.dedup();
    out
}

fn user_in_use(state: &AppState) -> bool {
    let (_, source) = state.policy();
    match policy::user_file() {
        Some(file) => source == "built-in defaults" || source == file.display().to_string(),
        None => false,
    }
}

pub async fn policy_view(State(state): State<Arc<AppState>>) -> Response {
    let file = policy::user_file();
    let policy = match file.as_deref().filter(|f| f.is_file()) {
        Some(f) => match Policy::load(f) {
            Ok(p) => p,
            Err(e) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        },
        None => Policy::default(),
    };
    axum::Json(PolicyView {
        source: state.policy().1,
        user_file: file.as_deref().map(display_path),
        user_in_use: user_in_use(&state),
        policy,
        entities: entities(),
        locales: veilq_core::detect::LOCALES,
        actions: vec!["mask", "pseudonymize", "fake", "remove"],
        validators: veilq_core::detect::validate::VALIDATORS,
        purposes: purposes(),
    })
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyTry {
    policy: Policy,
    /// A sample to run the policy's patterns on.
    #[serde(default)]
    sample: Option<String>,
}

#[derive(Serialize)]
pub struct Tried {
    /// The file the policy would be.
    toml: String,
    /// The sample with its findings replaced (`mask`), and what was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<Vec<String>>,
}

/// The policy checked as its file, written out, and tried on a sample with
/// the patterns alone (no models: quick, and the same for every sample).
fn try_policy(request: &PolicyTry) -> Result<(Policy, Tried), String> {
    let toml = request.policy.to_toml()?;
    let mut policy = Policy::parse(&toml)?;
    // Deny files are relative to the user's policy file (ADR-0044).
    if let Some(dir) = policy::user_file()
        .as_deref()
        .and_then(std::path::Path::parent)
    {
        policy.read_deny_files(dir)?;
    }
    let (output, findings) = match &request.sample {
        Some(sample) => {
            let engine = Engine::new(&policy, None, b"sample", Some(Action::Mask))
                .map_err(|e| e.to_string())?;
            let r = engine
                .redact_str(sample, Format::Text)
                .map_err(|e| e.to_string())?;
            let found = r
                .report
                .findings
                .iter()
                .map(|f| format!("{} ({})", f.entity.as_str(), f.detector))
                .collect();
            (Some(r.text), Some(found))
        }
        None => (None, None),
    };
    Ok((
        policy,
        Tried {
            toml,
            output,
            findings,
        },
    ))
}

pub async fn policy_try(axum::Json(request): axum::Json<PolicyTry>) -> Response {
    match try_policy(&request) {
        Ok((_, tried)) => axum::Json(tried).into_response(),
        Err(e) => api_error(StatusCode::UNPROCESSABLE_ENTITY, e),
    }
}

#[derive(Serialize)]
pub struct Saved {
    path: String,
    /// Whether runs use it now; else a project's `veilq.toml` comes first.
    in_use: bool,
}

/// Writes the user's policy (checked first) and, where it is the policy in
/// use, builds the engine with it.
pub async fn policy_save(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<PolicyTry>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Saved, (StatusCode, String)> {
        let unprocessable = |e: String| (StatusCode::UNPROCESSABLE_ENTITY, e);
        let (policy, tried) = try_policy(&request).map_err(unprocessable)?;
        let file = policy::user_file().ok_or_else(|| {
            unprocessable("this system names no folder for a user's policy".into())
        })?;
        let in_use = user_in_use(&state);
        if in_use {
            // Built before it is written: a policy the engine refuses (a
            // detector it requires is not installed) is not saved.
            let engine = match &state.reload {
                Some(reload) => Some(reload(&policy).map_err(|e| unprocessable(format!("{e:#}")))?),
                None => None,
            };
            write_atomically(&file, tried.toml.as_bytes()).map_err(unprocessable)?;
            if let Some(engine) = engine
                && let Ok(mut e) = state.engine.write()
            {
                *e = engine;
            }
            if let Ok(mut p) = state.policy.write() {
                *p = (policy, file.display().to_string());
            }
        } else {
            write_atomically(&file, tried.toml.as_bytes()).map_err(unprocessable)?;
        }
        Ok(Saved {
            path: display_path(&file),
            in_use,
        })
    })
    .await;
    match result {
        Ok(Ok(saved)) => axum::Json(saved).into_response(),
        Ok(Err((status, e))) => api_error(status, e),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn write_atomically(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = file.parent().ok_or("a file without a folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = file.with_extension("toml.veilq-tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("{}: {e}", file.display()))
}

// ---- folders --------------------------------------------------------------

#[derive(Deserialize)]
pub struct FoldersQuery {
    /// An absolute folder; empty: the starting points.
    #[serde(default)]
    path: String,
}

#[derive(Serialize)]
pub struct Folders {
    /// The folder listed, absolute; empty for the starting points.
    path: String,
    parent: Option<String>,
    folders: Vec<FolderEntry>,
}

#[derive(Serialize)]
struct FolderEntry {
    name: String,
    path: String,
}

/// Home, Documents, Desktop and (on Windows) the drives.
fn starting_points() -> Vec<FolderEntry> {
    let mut out = Vec::new();
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if let Some(home) = &home {
        out.push(FolderEntry {
            name: "Home".into(),
            path: display_path(home),
        });
        for sub in ["Documents", "Desktop", "Downloads"] {
            let p = home.join(sub);
            if p.is_dir() {
                out.push(FolderEntry {
                    name: sub.into(),
                    path: display_path(&p),
                });
            }
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        if Path::new(&drive).is_dir() {
            out.push(FolderEntry {
                name: drive.clone(),
                path: drive,
            });
        }
    }
    #[cfg(not(windows))]
    out.push(FolderEntry {
        name: "/".into(),
        path: "/".into(),
    });
    out
}

/// The sub-folders of a folder, names only: no file is read here.
pub async fn folders(Query(query): Query<FoldersQuery>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Folders, String> {
        if query.path.is_empty() {
            return Ok(Folders {
                path: String::new(),
                parent: None,
                folders: starting_points(),
            });
        }
        let dir = PathBuf::from(&query.path);
        if !dir.is_absolute() {
            return Err(format!("{}: not an absolute folder", query.path));
        }
        let mut folders: Vec<FolderEntry> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (!name.starts_with('.') && !name.starts_with('$')).then(|| FolderEntry {
                    path: display_path(&e.path()),
                    name,
                })
            })
            .collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        Ok(Folders {
            path: display_path(&dir),
            parent: dir.parent().map(display_path),
            folders,
        })
    })
    .await;
    match result {
        Ok(Ok(f)) => axum::Json(f).into_response(),
        Ok(Err(e)) => api_error(StatusCode::UNPROCESSABLE_ENTITY, e),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootRequest {
    path: String,
}

/// Makes `path` the root the UI reads under; a double-click veilq keeps it
/// for its next start.
pub async fn set_root(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<RootRequest>,
) -> Response {
    let dir = match PathBuf::from(&request.path).canonicalize() {
        Ok(d) if d.is_dir() => d,
        Ok(_) => {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("{}: not a folder", request.path),
            );
        }
        Err(e) => {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("{}: {e}", request.path),
            );
        }
    };
    if state.desktop {
        let mut settings = crate::desktop::Settings::load();
        settings.root = Some(dir.clone());
        if let Err(e) = settings.save() {
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}"));
        }
    }
    if let Ok(mut r) = state.root.write() {
        *r = dir.clone();
    }
    axum::Json(serde_json::json!({ "root": display_path(&dir) })).into_response()
}

// ---- a folder run -----------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchRequest {
    /// A folder under the root, `/`-separated; empty: the root.
    source: String,
    /// A new or empty folder, absolute, outside the source.
    target: String,
    #[serde(default)]
    action: Option<Action>,
    /// Pictures whose verify rounds do not settle are written for a review
    /// (ADR-0026), instead of being listed as errors.
    #[serde(default)]
    unverified: bool,
}

/// One file of a folder run, as its report lists it: no values.
#[derive(Clone, Serialize)]
pub struct BatchFile {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    types: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    review: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Batch {
    source: String,
    target: String,
    total: usize,
    done: usize,
    current: Option<String>,
    running: bool,
    cancelled: bool,
    files: Vec<BatchFile>,
}

pub struct BatchJob {
    pub(super) state: Batch,
    cancel: Arc<AtomicBool>,
}

/// The files under `dir`, relative and `/`-separated; `.git` and links are
/// left out.
fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let t = e.file_type()?;
        if t.is_symlink() || e.file_name() == ".git" {
            continue;
        }
        if t.is_dir() {
            walk(&e.path(), base, out)?;
        } else if t.is_file() {
            let rel = e
                .path()
                .strip_prefix(base)
                .unwrap_or(&e.path())
                .to_path_buf();
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Redacts one file to `out`; its findings' types, and whether a review is
/// asked for.
fn redact_file(
    engine: &Engine,
    input: &Path,
    out: &Path,
) -> Result<(usize, Vec<String>, bool), String> {
    // The line names the file already: its absolute path is left out.
    let format = Format::from_path(input).map_err(|e| {
        e.to_string()
            .replace(&format!(" ({})", input.display()), "")
    })?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = out.with_extension("veilq-tmp");
    let run = (|| -> Result<veilq_core::engine::Run, String> {
        if format.is_container() {
            let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
            let (bytes_out, run) = engine
                .run_bytes(&bytes, format, true)
                .map_err(|e| e.to_string())?;
            std::fs::write(&tmp, bytes_out).map_err(|e| e.to_string())?;
            Ok(run)
        } else {
            let reader =
                std::io::BufReader::new(std::fs::File::open(input).map_err(|e| e.to_string())?);
            let mut file =
                std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
            let run = engine
                .run(reader, Some(&mut file), format)
                .map_err(|e| e.to_string())?;
            std::io::Write::flush(&mut file).map_err(|e| e.to_string())?;
            Ok(run)
        }
    })();
    let run = match run {
        Ok(r) => r,
        Err(e) => {
            // Fail closed: nothing of a file veilq could not redact is left.
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    let mut types: Vec<String> = run
        .report
        .findings
        .iter()
        .map(|f| f.entity.as_str().to_owned())
        .collect();
    types.sort();
    types.dedup();
    Ok((run.report.findings.len(), types, run.report.review_required))
}

pub async fn batch_start(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<BatchRequest>,
) -> Response {
    if let Ok(job) = state.batch.lock()
        && job.as_ref().is_some_and(|j| j.state.running)
    {
        return api_error(StatusCode::CONFLICT, "a folder run is running");
    }
    let root = state.root();
    let source = match root::resolve_under(&root, Path::new(&request.source)) {
        Ok(p) if p.is_dir() => p,
        Ok(_) => {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("{}: not a folder", request.source),
            );
        }
        Err(e) => return api_error(StatusCode::FORBIDDEN, e.to_string()),
    };
    let target = PathBuf::from(&request.target);
    if !target.is_absolute() {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the target must be an absolute folder",
        );
    }
    let target_canon = target.canonicalize().unwrap_or_else(|_| {
        target
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join(target.file_name().unwrap_or_default()))
            .unwrap_or(target.clone())
    });
    if target_canon.starts_with(&source) {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the target must not be inside the source",
        );
    }
    if target.is_dir()
        && std::fs::read_dir(&target)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("{}: not empty; choose a new folder", request.target),
        );
    }
    let mut files = Vec::new();
    if let Err(e) = walk(&source, &source, &mut files) {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("{}: {e}", source.display()),
        );
    }
    // The engine is taken in the run's thread: it waits there for the
    // models, not here (ADR-0032).
    let (action, unverified) = (request.action, request.unverified);
    let cancel = Arc::new(AtomicBool::new(false));
    let initial = Batch {
        source: display_path(&source),
        target: display_path(&target),
        total: files.len(),
        done: 0,
        current: None,
        running: true,
        cancelled: false,
        files: Vec::new(),
    };
    if let Ok(mut job) = state.batch.lock() {
        *job = Some(BatchJob {
            state: initial.clone(),
            cancel: cancel.clone(),
        });
    }
    let shared = state.clone();
    std::thread::spawn(move || {
        let engine = shared.ready_engine().and_then(|e| match action {
            None => Ok(e),
            Some(a) => e.with_action(Some(a)).map_err(super::Failure::from),
        });
        let engine = match engine {
            Ok(e) => e.with_unverified(unverified),
            Err(super::Failure(_, e)) => {
                if let Ok(mut job) = shared.batch.lock()
                    && let Some(j) = job.as_mut()
                {
                    j.state.running = false;
                    j.state.files.push(BatchFile {
                        path: String::new(),
                        findings: None,
                        types: Vec::new(),
                        review: false,
                        error: Some(e),
                    });
                }
                return;
            }
        };
        let update = |f: &dyn Fn(&mut Batch)| {
            if let Ok(mut job) = shared.batch.lock()
                && let Some(j) = job.as_mut()
            {
                f(&mut j.state);
            }
        };
        let mut report = Vec::new();
        for rel in &files {
            if cancel.load(Ordering::Relaxed) {
                update(&|b| b.cancelled = true);
                break;
            }
            update(&|b| b.current = Some(rel.clone()));
            let entry = match redact_file(&engine, &source.join(rel), &target.join(rel)) {
                Ok((n, types, review)) => BatchFile {
                    path: rel.clone(),
                    findings: Some(n),
                    types,
                    review,
                    error: None,
                },
                Err(e) => BatchFile {
                    path: rel.clone(),
                    findings: None,
                    types: Vec::new(),
                    review: false,
                    error: Some(e),
                },
            };
            report.push(entry.clone());
            update(&|b| {
                b.done += 1;
                b.files.push(entry.clone());
            });
        }
        let _ = std::fs::create_dir_all(&target);
        let _ = std::fs::write(
            target.join("veilq-report.json"),
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        );
        update(&|b| {
            b.running = false;
            b.current = None;
        });
    });
    axum::Json(initial).into_response()
}

pub async fn batch_status(State(state): State<Arc<AppState>>) -> Response {
    match state
        .batch
        .lock()
        .ok()
        .and_then(|j| j.as_ref().map(|j| j.state.clone()))
    {
        Some(b) => axum::Json(b).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

pub async fn batch_cancel(State(state): State<Arc<AppState>>) -> StatusCode {
    if let Ok(job) = state.batch.lock()
        && let Some(j) = job.as_ref()
    {
        j.cancel.store(true, Ordering::Relaxed);
    }
    StatusCode::NO_CONTENT
}
