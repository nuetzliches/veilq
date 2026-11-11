//! The UI's settings (ADR-0027): setup (install, Start menu, desktop,
//! PATH), the models, quitting, and a desktop window's sign of life.
//!
//! Everything here writes only what the user's page asked for, behind the
//! same token and `Host`/`Origin` checks as the rest of the API: the user's
//! own shortcuts, PATH and "Installed apps" entry, and models whose SHA-256
//! is compiled into the binary (ADR-0005).

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use veilq_core::models;

use super::{AppState, api_error};
use crate::desktop;
use crate::fetch::Progress;

/// A model download started from the UI.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Job {
    Running(Progress),
    Done,
    Failed { error: String },
}

pub async fn ping(State(state): State<Arc<AppState>>) -> StatusCode {
    if let Ok(mut t) = state.last_ping.lock() {
        *t = Instant::now();
    }
    StatusCode::NO_CONTENT
}

pub async fn quit(State(state): State<Arc<AppState>>) -> StatusCode {
    state.shutdown.notify_one();
    StatusCode::NO_CONTENT
}

pub async fn status() -> axum::Json<desktop::Status> {
    axum::Json(desktop::status())
}

pub async fn install(axum::Json(request): axum::Json<desktop::Install>) -> Response {
    match tokio::task::spawn_blocking(move || desktop::install(&request)).await {
        Ok(Ok(status)) => axum::Json(status).into_response(),
        Ok(Err(e)) => api_error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Uninstalls, then ends the server: the program is removed once it ended.
pub async fn uninstall(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<desktop::Uninstall>,
) -> Response {
    match tokio::task::spawn_blocking(move || desktop::uninstall(&request)).await {
        Ok(Ok(())) => {
            state.shutdown.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(e)) => api_error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Serialize)]
pub struct Component {
    name: &'static str,
    /// What it adds, its source and licence, its size: as the README says.
    about: &'static str,
    installed: bool,
    /// Whether this release pins files for it on this platform.
    available: bool,
    /// Whether the running engine uses it (after a fetch, once reloaded).
    active: bool,
    files: Vec<File>,
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<Job>,
}

#[derive(Serialize)]
struct File {
    name: &'static str,
    url: String,
    installed: bool,
}

#[derive(Serialize)]
pub struct Models {
    dir: String,
    components: Vec<Component>,
}

fn about(component: &str) -> &'static str {
    match component {
        "ner" => {
            "Names that no word marks (`Beim Termin war auch Lena Vogt`). GLiNER multi PII (Apache-2.0) in ONNX, with ONNX Runtime (MIT). About 600 MB."
        }
        "ocr" => {
            "Text in images, GIFs and PDF pages; without it they are refused. PaddleOCR in ONNX (Apache-2.0), with ONNX Runtime (MIT). About 45 MB."
        }
        "pdf" => "PDF files, page by page (needs OCR too). PDFium (BSD-3-Clause). About 7 MB.",
        _ => "",
    }
}

pub async fn models(State(state): State<Arc<AppState>>) -> axum::Json<Models> {
    let dir = models::model_dir();
    let engine = state.engine();
    let jobs = state.jobs.lock().map(|j| j.clone()).unwrap_or_default();
    let components = models::COMPONENTS
        .iter()
        .map(|&name| {
            let pinned = models::pinned_for(name);
            Component {
                name,
                about: about(name),
                installed: models::installed(name, &dir),
                available: !pinned.is_empty(),
                active: match name {
                    "ner" => engine.has_ner(),
                    "ocr" => engine.has_ocr(),
                    "pdf" => engine.has_pdf(),
                    _ => false,
                },
                files: pinned
                    .iter()
                    .map(|f| File {
                        name: f.name,
                        url: f.url(),
                        installed: dir.join(f.file).is_file(),
                    })
                    .collect(),
                job: jobs.get(name).cloned(),
            }
        })
        .collect();
    axum::Json(Models {
        dir: super::display_path(&dir),
        components,
    })
}

/// Starts fetching `component` in the background; `GET /models` shows how
/// far it is. When it is done, the engine is built again with it.
pub async fn fetch(
    State(state): State<Arc<AppState>>,
    UrlPath(component): UrlPath<String>,
) -> Response {
    let Some(&name) = models::COMPONENTS.iter().find(|c| **c == component) else {
        return api_error(StatusCode::NOT_FOUND, format!("no component `{component}`"));
    };
    {
        let Ok(mut jobs) = state.jobs.lock() else {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "a poisoned lock");
        };
        if matches!(jobs.get(name), Some(Job::Running(_))) {
            return StatusCode::ACCEPTED.into_response();
        }
        jobs.insert(name.to_owned(), Job::Running(Progress::default()));
    }
    std::thread::spawn(move || {
        let set = |job: Job| {
            if let Ok(mut jobs) = state.jobs.lock() {
                jobs.insert(name.to_owned(), job);
            }
        };
        let result = crate::fetch::component(name, &models::model_dir(), &|p| {
            set(Job::Running(p.clone()))
        })
        .and_then(|()| match &state.reload {
            Some(reload) => {
                let engine = reload()?;
                if let Ok(mut e) = state.engine.write() {
                    *e = engine;
                }
                Ok(())
            }
            None => Ok(()),
        });
        set(match result {
            Ok(()) => Job::Done,
            Err(e) => Job::Failed {
                error: format!("{e:#}"),
            },
        });
    });
    StatusCode::ACCEPTED.into_response()
}

/// Whether a newer release is out: GitHub's API is asked, nothing else is
/// sent (ADR-0027, point 6).
pub async fn update_check() -> Response {
    match tokio::task::spawn_blocking(crate::update::check).await {
        Ok(Ok(found)) => axum::Json(found).into_response(),
        Ok(Err(e)) => api_error(StatusCode::BAD_GATEWAY, format!("{e:#}")),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Installs the newest release where this program is and starts it; this
/// server ends. Only for a veilq a double click started: a `veilq ui` of a
/// shell (or of a build tool) is updated with `veilq update`.
pub async fn update_install(State(state): State<Arc<AppState>>) -> Response {
    if !state.desktop {
        return api_error(
            StatusCode::CONFLICT,
            "this veilq runs from a shell: update it with `veilq update`",
        );
    }
    match tokio::task::spawn_blocking(|| {
        let (found, program) = crate::update::install_latest()?;
        crate::update::restart(&program)?;
        anyhow::Ok(found)
    })
    .await
    {
        Ok(Ok(found)) => {
            state.shutdown.notify_one();
            axum::Json(found).into_response()
        }
        Ok(Err(e)) => api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
