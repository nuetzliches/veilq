//! `veilq ui`: a local web UI that sets the input beside the output
//! (ADR-0008).
//!
//! The server listens on loopback only. Every API request carries a token the
//! server made at start and handed to the browser in the URL's fragment, and
//! every request names this server in `Host` (and `Origin`, when sent): a web
//! page in the same browser, or one that rebinds its DNS name to 127.0.0.1,
//! gets nothing. Files are read under one root, as the MCP server reads them
//! (ADR-0007). The HTTP API is internal to the embedded UI, not a contract.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use veilq_core::{Action, Engine, Error, Format, Policy, Report, root};

#[path = "ui_settings.rs"]
mod settings;
#[path = "ui_setup.rs"]
mod setup;

static UI: Dir<'_> = include_dir!("$OUT_DIR/ui");

/// Builds the engine for a policy: after a model was installed or the
/// user's policy saved from the UI.
pub type Reload = Box<dyn Fn(&Policy) -> anyhow::Result<Engine> + Send + Sync>;

/// Builds the engine with its models, once, beside the server (ADR-0032).
pub type Load = Box<dyn FnOnce() -> anyhow::Result<Engine> + Send>;

/// Whether the engine has its models yet.
#[derive(Clone)]
enum ModelState {
    Ready,
    Loading,
    Failed(String),
}

/// How long a run waits for the models before it gives up.
const LOAD_WAIT: Duration = Duration::from_secs(600);

/// A UI started by a double click ends this long after its window last
/// said it is open (a background tab's timers run once a minute).
const IDLE: Duration = Duration::from_secs(180);

/// The largest input the UI redacts; a log export above this belongs to
/// `veilq redact`, which streams.
const MAX_INPUT: usize = 32 << 20;
/// A request carries an upload as base64 (4/3) and some JSON around it.
const MAX_BODY: usize = MAX_INPUT / 3 * 4 + (64 << 10);
/// Entries a directory listing returns at most.
const MAX_ENTRIES: usize = 5000;

pub struct Options {
    pub root: PathBuf,
    pub port: u16,
    /// The port of a reverse proxy on loopback that serves this server
    /// under its own address (devstack), from `--origin`.
    pub proxy_port: Option<u16>,
    /// Where the token is kept across starts (ADR-0021); `None`: a new one
    /// per start.
    pub token_file: Option<PathBuf>,
    pub open: bool,
    pub policy: Policy,
    pub policy_source: String,
    pub engine: Engine,
    /// Loads the models beside the server, which answers with `engine` (its
    /// patterns) meanwhile; a run waits for them, it does not go without
    /// (ADR-0032). `None`: `engine` is the one.
    pub load: Option<Load>,
    /// Started by a double click (ADR-0027): the URL opens in an app
    /// window, the running instance is written down, and the server ends
    /// when its window has been gone for a while.
    pub desktop: bool,
    pub reload: Option<Reload>,
    /// A window of veilq's own shows the UI (ADR-0029): it is told the URL,
    /// and closing it ends veilq; no browser window, no heartbeat.
    pub window: Option<WindowHooks>,
}

/// What a window of veilq's own needs from the server.
pub struct WindowHooks {
    /// Gets the URL once the server listens.
    pub ready: std::sync::mpsc::Sender<String>,
    /// Brings the window to the front: a second double click asks for it.
    pub focus: Box<dyn Fn() + Send + Sync>,
}

struct AppState {
    token: String,
    hosts: Vec<String>,
    origins: Vec<String>,
    /// Canonical; the settings page can choose another.
    root: RwLock<PathBuf>,
    /// The policy and where it came from; saving the user's replaces it.
    policy: RwLock<(Policy, String)>,
    /// Replaced when a model is installed from the UI.
    engine: RwLock<Engine>,
    reload: Option<Reload>,
    desktop: bool,
    /// Ends the server: the UI's "Quit", or a desktop window gone.
    shutdown: tokio::sync::Notify,
    last_ping: Mutex<Instant>,
    /// Model downloads started from the UI, by component.
    jobs: Mutex<HashMap<String, setup::Job>>,
    /// The folder run, while it runs and after.
    batch: Mutex<Option<settings::BatchJob>>,
    /// The window of veilq's own, brought to the front on request.
    focus: Option<Box<dyn Fn() + Send + Sync>>,
    /// The models loading beside the server, and who waits for them.
    loading: (Mutex<ModelState>, std::sync::Condvar),
}

pub fn run(options: Options) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve(options))
}

async fn serve(options: Options) -> anyhow::Result<()> {
    let root = options
        .root
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("root {}: {e}", options.root.display()))?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, options.port)))
            .await?;
    let port = listener.local_addr()?.port();
    let ports: Vec<u16> = std::iter::once(port).chain(options.proxy_port).collect();
    let mut state = AppState::new(
        token(options.token_file.as_deref())?,
        &ports,
        root,
        options.policy,
        options.policy_source,
        options.engine,
    );
    state.desktop = options.desktop;
    state.reload = options.reload;
    let ready = options.window.map(|w| {
        state.focus = Some(w.focus);
        w.ready
    });
    if options.load.is_some() {
        state.loading = (Mutex::new(ModelState::Loading), std::sync::Condvar::new());
    }
    let state = Arc::new(state);
    if let Some(load) = options.load {
        let loader = state.clone();
        std::thread::spawn(move || {
            let result = load();
            let (lock, ready) = &loader.loading;
            let next = match result {
                Ok(engine) => {
                    if let Ok(mut e) = loader.engine.write() {
                        *e = engine;
                    }
                    ModelState::Ready
                }
                Err(e) => ModelState::Failed(format!("{e:#}")),
            };
            if let Ok(mut s) = lock.lock() {
                *s = next;
            }
            ready.notify_all();
        });
    }

    let shown = options.proxy_port.unwrap_or(port);
    let url = format!("http://127.0.0.1:{shown}/#token={}", state.token);
    println!("veilq ui: {url}");
    println!("root:     {}", display_path(&state.root()));
    println!("policy:   {}", state.policy().1);
    println!("Ctrl+C stops it.");
    let instance = crate::desktop::instance_file();
    if state.desktop {
        if let Some(dir) = instance.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(
            &instance,
            serde_json::to_string(&crate::desktop::Instance {
                url: url.clone(),
                port,
                pid: std::process::id(),
            })?,
        )?;
        crate::desktop::hide_console();
        if let Some(ready) = ready {
            // veilq's own window: closing it ends veilq.
            let _ = ready.send(url.clone());
        } else {
            crate::desktop::open_app_window(&url);
            // The window says it is open; once it has been gone a while, end.
            let watch = state.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let idle = watch
                        .last_ping
                        .lock()
                        .map(|t| t.elapsed())
                        .unwrap_or_default();
                    if idle > IDLE {
                        watch.shutdown.notify_one();
                        break;
                    }
                }
            });
        }
    } else if options.open {
        open_browser(&url);
    }

    let ended = state.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = ended.shutdown.notified() => {}
            }
        })
        .await?;
    if options.desktop {
        let _ = std::fs::remove_file(&instance);
    }
    Ok(())
}

impl AppState {
    /// `ports`: this server's and a proxy's in front of it.
    fn new(
        token: String,
        ports: &[u16],
        root: PathBuf,
        policy: Policy,
        policy_source: String,
        engine: Engine,
    ) -> Self {
        let hosts: Vec<String> = ports
            .iter()
            .flat_map(|p| [format!("127.0.0.1:{p}"), format!("localhost:{p}")])
            .collect();
        Self {
            token,
            origins: hosts.iter().map(|h| format!("http://{h}")).collect(),
            hosts,
            root: RwLock::new(root),
            policy: RwLock::new((policy, policy_source)),
            engine: RwLock::new(engine),
            reload: None,
            focus: None,
            desktop: false,
            shutdown: tokio::sync::Notify::new(),
            last_ping: Mutex::new(Instant::now()),
            jobs: Mutex::new(HashMap::new()),
            batch: Mutex::new(None),
            loading: (Mutex::new(ModelState::Ready), std::sync::Condvar::new()),
        }
    }

    fn root(&self) -> PathBuf {
        read(&self.root)
    }

    fn policy(&self) -> (Policy, String) {
        read(&self.policy)
    }

    /// The engine with its models, for a run: waits while they load (a run
    /// does not go with the patterns alone, ADR-0032). Blocking: call it
    /// from a blocking task.
    fn ready_engine(&self) -> Result<Engine, Failure> {
        let (lock, ready) = &self.loading;
        let state = lock
            .lock()
            .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "a poisoned lock".into()))?;
        let (state, timeout) = ready
            .wait_timeout_while(state, LOAD_WAIT, |s| matches!(s, ModelState::Loading))
            .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "a poisoned lock".into()))?;
        match &*state {
            ModelState::Ready => Ok(self.engine()),
            ModelState::Failed(e) => Err(Failure(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("the models did not load: {e}"),
            )),
            ModelState::Loading if timeout.timed_out() => Err(Failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "the models are still loading".into(),
            )),
            ModelState::Loading => unreachable!("waited while loading"),
        }
    }

    fn loading(&self) -> ModelState {
        self.loading
            .0
            .lock()
            .map(|s| s.clone())
            .unwrap_or(ModelState::Ready)
    }

    /// The engine as it is now (a model installed from the UI replaces it).
    fn engine(&self) -> Engine {
        self.engine
            .read()
            .map(|e| e.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }
}

/// A clone of what `lock` holds, also after a panic poisoned it.
fn read<T: Clone>(lock: &RwLock<T>) -> T {
    lock.read()
        .map(|v| v.clone())
        .unwrap_or_else(|p| p.into_inner().clone())
}

/// The port of `--origin`: an `http` URL on loopback, `http://127.0.0.1:4760`
/// or `http://localhost:4760`. Anything else would let another host's pages
/// use the API.
pub fn parse_origin(origin: &str) -> Result<u16, String> {
    let error = || {
        format!("--origin {origin}: expected http://127.0.0.1:<port> or http://localhost:<port>")
    };
    let rest = origin.trim_end_matches('/');
    let rest = rest
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| rest.strip_prefix("http://localhost:"))
        .ok_or_else(error)?;
    match rest.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(error()),
    }
}

fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/info", get(info))
        .route("/files", get(files))
        .route("/redact", post(redact))
        .route("/picture", get(picture))
        .route("/ping", post(setup::ping))
        .route("/quit", post(setup::quit))
        .route("/focus", post(setup::focus))
        .route("/setup", get(setup::status).post(setup::install))
        .route("/setup/uninstall", post(setup::uninstall))
        .route(
            "/update",
            get(setup::update_check).post(setup::update_install),
        )
        .route("/models", get(setup::models))
        .route(
            "/policy",
            get(settings::policy_view).put(settings::policy_save),
        )
        .route("/policy/try", post(settings::policy_try))
        .route("/folders", get(settings::folders))
        .route("/root", axum::routing::put(settings::set_root))
        .route(
            "/batch",
            get(settings::batch_status).post(settings::batch_start),
        )
        .route("/batch/cancel", post(settings::batch_cancel))
        .route("/models/{component}/fetch", post(setup::fetch))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api", api)
        .fallback(asset)
        .layer(middleware::from_fn_with_state(state.clone(), require_host))
        .with_state(state)
}

/// 32 random bytes as hex.
/// The token of `file` if it holds one, else a new one, written there
/// (ADR-0021); without a file, a new one.
fn token(file: Option<&Path>) -> anyhow::Result<String> {
    let Some(file) = file else {
        return new_token();
    };
    if file.is_file() {
        let t = std::fs::read_to_string(file)?.trim().to_owned();
        if t.len() < 32 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
            anyhow::bail!(
                "{}: not a token (64 hex characters); delete it for a new one",
                file.display()
            );
        }
        return Ok(t);
    }
    let t = new_token()?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, format!("{t}\n"))?;
    Ok(t)
}

fn new_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no randomness for the token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

// ------------------------------------------------------------------ guards

/// Refuses a request that does not name this server: DNS rebinding sends
/// another `Host`, a page of another site sends its own `Origin`.
async fn require_host(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let host_ok =
        header_str(headers, header::HOST).is_some_and(|h| state.hosts.iter().any(|a| a == h));
    let origin_ok = match headers.get(header::ORIGIN) {
        None => true,
        Some(o) => o
            .to_str()
            .is_ok_and(|o| state.origins.iter().any(|a| a == o)),
    };
    if !(host_ok && origin_ok) {
        return api_error(
            StatusCode::FORBIDDEN,
            "request does not come from this server's page",
        );
    }
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; style-src 'self'; \
             connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    response
}

async fn require_token(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let given = header_str(request.headers(), header::AUTHORIZATION)
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(given.as_bytes(), state.token.as_bytes()) {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "missing or wrong token: open the URL veilq ui printed",
        );
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ------------------------------------------------------------------ API

#[derive(Serialize)]
struct ApiError {
    error: String,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        axum::Json(ApiError {
            error: message.into(),
        }),
    )
        .into_response()
}

/// An API error: a status and one sentence.
struct Failure(StatusCode, String);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        api_error(self.0, self.1)
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        let status = match &e {
            Error::OutsideRoot { .. } => StatusCode::FORBIDDEN,
            Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                StatusCode::NOT_FOUND
            }
            Error::NotBuilt(_) | Error::ActionNotBuilt(_) => StatusCode::NOT_IMPLEMENTED,
            // The UI offers to write it anyway (`unverified`).
            Error::Unsettled { .. } => StatusCode::CONFLICT,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        // Without Windows' `\\?\` prefix, which `canonicalize` adds.
        Self(status, e.to_string().replace(r"\\?\", ""))
    }
}

#[derive(Serialize)]
struct Info {
    version: &'static str,
    root: String,
    policy: String,
    default_action: &'static str,
    /// The actions a request can choose instead of the policy's;
    /// `pseudonymize` only with a key.
    actions: Vec<&'static str>,
    /// The policy's default threshold, where the output's slider starts
    /// (ADR-0046), and the highest a run may set.
    min_confidence: f32,
    max_min_confidence: f32,
    max_input: usize,
    /// Started by a double click: the UI says it is open, offers to quit,
    /// and opens setup on the first start.
    desktop: bool,
    /// Whether this platform's setup is built (ADR-0027) and offered: not
    /// in a package of the Microsoft Store (ADR-0039).
    setup: bool,
    /// Installed from the Microsoft Store: it updates and removes veilq
    /// (ADR-0039).
    store: bool,
    /// Shown in veilq's own window (ADR-0029): closing it ends veilq.
    window: bool,
    /// The models are still loading: a run waits for them (ADR-0032).
    loading: bool,
    /// Why the models did not load.
    #[serde(skip_serializing_if = "Option::is_none")]
    load_error: Option<String>,
}

async fn info(State(state): State<Arc<AppState>>) -> axum::Json<Info> {
    axum::Json(Info {
        version: env!("CARGO_PKG_VERSION"),
        root: display_path(&state.root()),
        policy: state.policy().1,
        default_action: state.policy().0.defaults.action.name(),
        actions: [
            Action::Mask,
            Action::Fake,
            Action::Remove,
            Action::Pseudonymize,
        ]
        .into_iter()
        .filter(|a| *a != Action::Pseudonymize || state.engine().has_key())
        .map(Action::name)
        .collect(),
        min_confidence: state.policy().0.defaults.min_confidence,
        max_min_confidence: veilq_core::engine::MAX_RUN_CONFIDENCE,
        max_input: MAX_INPUT,
        desktop: state.desktop,
        window: state.focus.is_some(),
        loading: matches!(state.loading(), ModelState::Loading),
        load_error: match state.loading() {
            ModelState::Failed(e) => Some(e),
            _ => None,
        },
        setup: crate::desktop::setup_available(),
        store: crate::package::packaged(),
    })
}

#[derive(Deserialize)]
struct FilesQuery {
    #[serde(default)]
    dir: String,
}

#[derive(Serialize)]
struct Listing {
    /// Relative to the root, with `/`; empty for the root.
    dir: String,
    entries: Vec<Entry>,
    truncated: bool,
}

#[derive(Serialize)]
struct Entry {
    name: String,
    path: String,
    dir: bool,
    size: u64,
}

async fn files(State(state): State<Arc<AppState>>, Query(query): Query<FilesQuery>) -> Response {
    match tokio::task::spawn_blocking(move || list(&state.root(), &query.dir)).await {
        Ok(Ok(listing)) => axum::Json(listing).into_response(),
        Ok(Err(e)) => Failure::from(e).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn list(root: &Path, dir: &str) -> Result<Listing, Error> {
    let resolved = root::resolve_under(root, Path::new(dir))?;
    let rel = relative(root, &resolved);
    let io = |source| Error::Io {
        path: PathBuf::from(dir),
        source,
    };
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(&resolved).map_err(io)? {
        let entry = entry.map_err(io)?;
        if entries.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        // Follows links; one that leads out of the root is listed, and
        // refused when it is opened.
        let Ok(meta) = entry.path().metadata() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        entries.push(Entry {
            name,
            path,
            dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
        });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    Ok(Listing {
        dir: rel,
        entries,
        truncated,
    })
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Source {
    /// Pasted text.
    Text { text: String },
    /// A file the browser read, base64.
    Upload { name: String, data: String },
    /// A file under the root.
    File { path: String },
}

#[derive(Deserialize)]
struct PictureQuery {
    path: String,
}

#[derive(Serialize)]
struct Picture {
    /// An image as it is; a PDF's first pages as PNG.
    pages: Vec<String>,
    media_type: &'static str,
    /// A PDF's number of pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    page_count: Option<usize>,
}

/// The pages a viewer shows of a PDF.
const MAX_PAGES: usize = 20;

/// An image or PDF under the root as it is, to show while it is redacted:
/// the answer of `/redact` holds it too, only later.
async fn picture(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PictureQuery>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Picture, Failure> {
        let path = Path::new(&query.path);
        let format = Format::from_path(path).ok().filter(|f| f.is_picture());
        let media_type = format.and_then(media_type).ok_or_else(|| {
            Failure(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("{}: not an image or PDF", query.path),
            )
        })?;
        let resolved = root::resolve_under(&state.root(), path).map_err(Failure::from)?;
        let io = |source| {
            Failure::from(Error::Io {
                path: PathBuf::from(&query.path),
                source,
            })
        };
        if resolved.metadata().map_err(io)?.len() > MAX_INPUT as u64 {
            return Err(too_large(&query.path));
        }
        let bytes = std::fs::read(&resolved).map_err(io)?;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        if format == Some(Format::Pdf) {
            let (pages, n) = state
                .ready_engine()?
                .pdf_pages(&bytes, MAX_PAGES)
                .map_err(Failure::from)?;
            return Ok(Picture {
                pages: pages.iter().map(|p| b64(p)).collect(),
                media_type: "image/png",
                page_count: Some(n),
            });
        }
        Ok(Picture {
            pages: vec![b64(&bytes)],
            media_type,
            page_count: None,
        })
    })
    .await;
    match result {
        Ok(Ok(p)) => axum::Json(p).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactRequest {
    source: Source,
    /// Overrides the policy's actions for this request, as `--action` does.
    #[serde(default)]
    action: Option<Action>,
    /// One threshold for this request in place of the policy's, as
    /// `--min-confidence` does (ADR-0046): the output's slider.
    #[serde(default)]
    min_confidence: Option<f32>,
    /// A picture whose verify rounds do not settle is written anyway, for a
    /// review, as `--unverified` does (ADR-0026).
    #[serde(default)]
    unverified: bool,
    /// Also return what veilq read and why it decided (ADR-0034): the
    /// values in clear, for this machine's UI only.
    #[serde(default)]
    explain: bool,
}

#[derive(Serialize)]
struct RedactResponse {
    /// The source's name: a file's path or upload name; absent for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    input: String,
    output: String,
    /// Per finding, its byte range in `input`.
    input_spans: Vec<[usize; 2]>,
    /// Per finding, the byte range of its replacement in `output`.
    output_spans: Vec<[usize; 2]>,
    report: Report,
    /// An Office file, image or PDF: no text to set side by side; the
    /// redacted file, base64.
    #[serde(skip_serializing_if = "Option::is_none")]
    output_base64: Option<String>,
    /// An image or PDF: the input as it came, base64, to set beside the
    /// output.
    #[serde(skip_serializing_if = "Option::is_none")]
    input_base64: Option<String>,
    /// An image or PDF: its media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<&'static str>,
    /// A PDF: its first pages before and after, as PNG, in the pixels its
    /// findings' regions are in, and its number of pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    input_pages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_pages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_count: Option<usize>,
    /// What veilq read and why it decided, when asked for (ADR-0034).
    #[serde(skip_serializing_if = "Option::is_none")]
    explain: Option<Vec<veilq_core::Reading>>,
}

/// The media type of a picture format.
fn media_type(format: Format) -> Option<&'static str> {
    Some(match format {
        Format::Png => "image/png",
        Format::Jpeg => "image/jpeg",
        Format::Webp => "image/webp",
        Format::Gif => "image/gif",
        Format::Pdf => "application/pdf",
        _ => return None,
    })
}

async fn redact(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<RedactRequest>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let engine = match request.action {
            None => state.ready_engine()?,
            Some(a) => state
                .ready_engine()?
                .with_action(Some(a))
                .map_err(Failure::from)?,
        }
        .with_min_confidence(request.min_confidence)
        .map_err(Failure::from)?
        .with_unverified(request.unverified);
        if !request.explain {
            return redact_source(&state, &engine, request.source);
        }
        let explanation = veilq_core::Explain::new();
        let engine = engine.with_explain(explanation.clone());
        let mut response = redact_source(&state, &engine, request.source)?;
        let mut readings = explanation.take();
        readings.dedup();
        response.explain = Some(readings);
        Ok(response)
    })
    .await;
    match result {
        Ok(Ok(response)) => axum::Json(response).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn redact_source(
    state: &AppState,
    engine: &Engine,
    source: Source,
) -> Result<RedactResponse, Failure> {
    let (name, bytes) = match source {
        Source::Text { text } => (None, text.into_bytes()),
        Source::Upload { name, data } => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| Failure(StatusCode::BAD_REQUEST, format!("{name}: {e}")))?;
            (Some(name), bytes)
        }
        Source::File { path } => {
            let resolved =
                root::resolve_under(&state.root(), Path::new(&path)).map_err(Failure::from)?;
            let size = resolved
                .metadata()
                .map_err(|source| {
                    Failure::from(Error::Io {
                        path: PathBuf::from(&path),
                        source,
                    })
                })?
                .len();
            if size > MAX_INPUT as u64 {
                return Err(too_large(&path));
            }
            let bytes = std::fs::read(&resolved).map_err(|source| {
                Failure::from(Error::Io {
                    path: PathBuf::from(&path),
                    source,
                })
            })?;
            (Some(path), bytes)
        }
    };
    let label = name.as_deref().unwrap_or("the text");
    if bytes.len() > MAX_INPUT {
        return Err(too_large(label));
    }
    let format = match &name {
        Some(n) => Format::from_path(Path::new(n)).map_err(Failure::from)?,
        None => Format::Text,
    };
    if format.is_container() {
        // Read whole (ADR-0019, ADR-0020). A mail is text: both sides are
        // shown, without marks (its findings name their parts). An Office
        // file is offered as a download.
        let (out, run) = engine
            .run_bytes(&bytes, format, true)
            .map_err(Failure::from)?;
        let mail = format == Format::Eml;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let (input_pages, output_pages, page_count) = if format == Format::Pdf {
            let (before, n) = engine.pdf_pages(&bytes, MAX_PAGES).map_err(Failure::from)?;
            let (after, _) = engine.pdf_pages(&out, MAX_PAGES).map_err(Failure::from)?;
            (
                Some(before.iter().map(|p| b64(p)).collect()),
                Some(after.iter().map(|p| b64(p)).collect()),
                Some(n),
            )
        } else {
            (None, None, None)
        };
        return Ok(RedactResponse {
            name,
            input: if mail {
                String::from_utf8_lossy(&bytes).into_owned()
            } else {
                String::new()
            },
            output: if mail {
                String::from_utf8_lossy(&out).into_owned()
            } else {
                String::new()
            },
            input_spans: Vec::new(),
            output_spans: Vec::new(),
            report: run.report,
            output_base64: (!mail).then(|| base64::engine::general_purpose::STANDARD.encode(&out)),
            input_base64: format
                .is_picture()
                .then(|| base64::engine::general_purpose::STANDARD.encode(&bytes)),
            media_type: media_type(format),
            input_pages,
            output_pages,
            page_count,
            explain: None,
        });
    }
    // Fail closed: what veilq cannot read is an error, not a pass-through.
    let input = String::from_utf8(bytes).map_err(|_| {
        Failure(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("{label} is not UTF-8 text"),
        )
    })?;
    let redacted = engine.redact_str(&input, format).map_err(Failure::from)?;
    let pairs = |spans: &[std::ops::Range<usize>]| spans.iter().map(|r| [r.start, r.end]).collect();
    Ok(RedactResponse {
        name,
        input_spans: pairs(&redacted.input_spans),
        output_spans: pairs(&redacted.output_spans),
        output: redacted.text,
        input,
        report: redacted.report,
        output_base64: None,
        input_base64: None,
        media_type: None,
        input_pages: None,
        output_pages: None,
        page_count: None,
        explain: None,
    })
}

fn too_large(label: &str) -> Failure {
    Failure(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!(
            "{label} is larger than {} MiB; use `veilq redact`",
            MAX_INPUT >> 20
        ),
    )
}

// ------------------------------------------------------------------ assets

async fn asset(request: Request) -> Response {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let file = UI.get_file(path).or_else(|| {
        // A client-side route: no extension in the last segment.
        let last = path.rsplit('/').next().unwrap_or(path);
        (!last.contains('.'))
            .then(|| UI.get_file("index.html"))
            .flatten()
    });
    let Some(file) = file else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let name = file.path().to_string_lossy();
    // Vite puts a content hash into every name under assets/.
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, content_type(&name)),
            (header::CACHE_CONTROL, cache),
        ],
        file.contents(),
    )
        .into_response()
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" | "webmanifest" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

// ------------------------------------------------------------------ helpers

/// `path` relative to `root`, with `/`.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Without Windows' `\\?\` prefix, which `canonicalize` adds.
fn display_path(path: &Path) -> String {
    let s = path.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(s)
}

pub(crate) fn open_browser(url: &str) {
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = result {
        eprintln!("could not open a browser ({e}); open the URL above");
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    const PORT: u16 = 4999;
    const PROXY_PORT: u16 = 4760;
    const TOKEN: &str = "test-token";

    fn app(root: &Path) -> Router {
        router(Arc::new(AppState::new(
            TOKEN.into(),
            &[PORT, PROXY_PORT],
            root.canonicalize().unwrap(),
            Policy::default(),
            "built-in defaults".into(),
            Engine::new(&Policy::default(), None, b"test", None).unwrap(),
        )))
    }

    /// A server whose models are still loading, and its state to end that.
    fn loading_app(root: &Path) -> (Router, Arc<AppState>) {
        let mut state = AppState::new(
            TOKEN.into(),
            &[PORT, PROXY_PORT],
            root.canonicalize().unwrap(),
            Policy::default(),
            "built-in defaults".into(),
            Engine::new(&Policy::default(), None, b"test", None).unwrap(),
        );
        state.loading = (Mutex::new(ModelState::Loading), std::sync::Condvar::new());
        let state = Arc::new(state);
        (router(state.clone()), state)
    }

    fn finish_loading(state: &AppState, to: ModelState) {
        *state.loading.0.lock().unwrap() = to;
        state.loading.1.notify_all();
    }

    #[tokio::test]
    async fn a_run_waits_for_the_models_and_never_goes_without_them() {
        let dir = tempfile::tempdir().unwrap();
        let redact = || {
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({ "source": { "kind": "text", "text": "Mail an mira.albers@example.com" } })
                        .to_string(),
                ))
                .unwrap()
        };
        // Loading: the info says so, and a run waits until the models are there.
        let (app, state) = loading_app(dir.path());
        let (_, info) = send(
            app.clone(),
            authed("GET", "/api/info").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(info["loading"], true);
        let waiter = tokio::spawn(send(app.clone(), redact()));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !waiter.is_finished(),
            "a run went before the models were there"
        );
        finish_loading(&state, ModelState::Ready);
        let (status, body) = waiter.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["output"], "Mail an <EMAIL_ADDRESS>");
        // Failed: no run with the patterns alone, the reason instead.
        let (app, state) = loading_app(dir.path());
        finish_loading(&state, ModelState::Failed("NER: no such file".into()));
        let (status, body) = send(app.clone(), redact()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        let (_, info) = send(app, authed("GET", "/api/info").body(Body::empty()).unwrap()).await;
        assert_eq!(info["loading"], false);
        assert!(info["load_error"].as_str().unwrap().contains("NER"));
    }

    fn request(method: &str, uri: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
    }

    fn authed(method: &str, uri: &str) -> axum::http::request::Builder {
        request(method, uri).header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }

    fn redact_body(source: serde_json::Value) -> Body {
        Body::from(serde_json::json!({ "source": source }).to_string())
    }

    #[tokio::test]
    async fn health_and_page_need_no_token() {
        let dir = tempfile::tempdir().unwrap();
        let (status, _) = send(
            app(dir.path()),
            request("GET", "/healthz").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let response = app(dir.path())
            .oneshot(request("GET", "/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert!(
            response
                .headers()
                .contains_key(header::CONTENT_SECURITY_POLICY)
        );
    }

    #[tokio::test]
    async fn another_host_or_origin_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        // DNS rebinding: the browser sends the attacker's name as Host.
        let rebound = Request::builder()
            .uri("/api/info")
            .header(header::HOST, format!("evil.example:{PORT}"))
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            send(app(dir.path()), rebound).await.0,
            StatusCode::FORBIDDEN
        );
        let cross_site = authed("GET", "/api/info")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            send(app(dir.path()), cross_site).await.0,
            StatusCode::FORBIDDEN
        );
        let same = authed("GET", "/api/info")
            .header(header::ORIGIN, format!("http://localhost:{PORT}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(app(dir.path()), same).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_api_needs_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let none = request("GET", "/api/info").body(Body::empty()).unwrap();
        assert_eq!(
            send(app(dir.path()), none).await.0,
            StatusCode::UNAUTHORIZED
        );
        let wrong = request("GET", "/api/info")
            .header(header::AUTHORIZATION, "Bearer test-tokem")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            send(app(dir.path()), wrong).await.0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn files_are_listed_under_the_root_only() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.log"), "a").unwrap();
        std::fs::write(root.join("b.txt"), "bb").unwrap();

        let (status, body) = send(
            app(&root),
            authed("GET", "/api/files").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["dir"], "");
        assert_eq!(body["entries"][0]["name"], "sub");
        assert_eq!(body["entries"][0]["dir"], true);
        assert_eq!(body["entries"][1]["path"], "b.txt");
        assert_eq!(body["entries"][1]["size"], 2);

        let (_, body) = send(
            app(&root),
            authed("GET", "/api/files?dir=sub")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(body["entries"][0]["path"], "sub/a.log");

        let (status, _) = send(
            app(&root),
            authed("GET", "/api/files?dir=..")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_file_outside_the_root_is_refused() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(outer.path().join("secret.txt"), "s").unwrap();
        let body = redact_body(serde_json::json!({ "kind": "file", "path": "../secret.txt" }));
        let (status, _) = send(
            app(&root),
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_run_can_choose_its_action() {
        let dir = tempfile::tempdir().unwrap();
        let text = "Hallo Erika Mustermann, Tel. 0421 5550173";
        let post = |action: &str| {
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({ "source": { "kind": "text", "text": text }, "action": action })
                        .to_string(),
                ))
                .unwrap()
        };
        let (status, body) = send(app(dir.path()), post("fake")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let out = body["output"].as_str().unwrap();
        assert!(out.starts_with("Hallo ") && !out.contains('<'), "{out}");
        assert!(
            !out.contains("Mustermann") && !out.contains("5550173"),
            "{out}"
        );
        let (status, body) = send(app(dir.path()), post("remove")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["output"], "Hallo , Tel. ");
        // Without a key there is no pseudonym, and the info does not offer it.
        let (status, _) = send(app(dir.path()), post("pseudonymize")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (_, info) = send(
            app(dir.path()),
            authed("GET", "/api/info").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(
            info["actions"],
            serde_json::json!(["mask", "fake", "remove"])
        );
    }

    #[tokio::test]
    async fn a_run_can_set_its_threshold_and_a_raised_one_asks_for_a_review() {
        let dir = tempfile::tempdir().unwrap();
        let post = |body: serde_json::Value| {
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let source = serde_json::json!({ "kind": "text", "text": "Tel. 0421 5550173" });
        let (status, body) = send(
            app(dir.path()),
            post(serde_json::json!({ "source": source, "min_confidence": 0.85 })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["report"]["review_required"], true);
        assert!(
            body["report"]["review_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r == "threshold raised from 70 % to 85 % for this run"),
            "{body}"
        );
        // Lower: no reason about the threshold.
        let (status, body) = send(
            app(dir.path()),
            post(serde_json::json!({ "source": source, "min_confidence": 0.5 })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let reasons = body["report"]["review_reasons"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            reasons
                .iter()
                .all(|r| !r.as_str().unwrap().contains("raised")),
            "{body}"
        );
        // Above the range: refused, no output.
        let (status, body) = send(
            app(dir.path()),
            post(serde_json::json!({ "source": source, "min_confidence": 1.0 })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.get("output").is_none());
        let (_, info) = send(
            app(dir.path()),
            authed("GET", "/api/info").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(info["min_confidence"], 0.7);
        assert_eq!(info["max_min_confidence"], 0.95);
    }

    #[test]
    fn an_unsettled_picture_is_a_conflict_the_ui_can_resolve() {
        let f = Failure::from(Error::Unsettled {
            entity: "PERSON".into(),
            at: "97x24 at 142,228".into(),
            rounds: 6,
        });
        assert_eq!(f.0, StatusCode::CONFLICT);
        let r: RedactRequest = serde_json::from_value(serde_json::json!({
            "source": { "kind": "text", "text": "x" },
            "unverified": true
        }))
        .unwrap();
        assert!(r.unverified);
    }

    #[tokio::test]
    async fn the_settings_api_needs_the_token_and_lists_the_models() {
        let dir = tempfile::tempdir().unwrap();
        for (method, uri) in [
            ("GET", "/api/models"),
            ("GET", "/api/setup"),
            ("POST", "/api/quit"),
        ] {
            let (status, _) = send(
                app(dir.path()),
                request(method, uri).body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
        let (status, body) = send(
            app(dir.path()),
            authed("GET", "/api/models").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let names: Vec<&str> = body["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["ner", "ocr", "face", "pdf"]);
        let (status, _) = send(
            app(dir.path()),
            authed("POST", "/api/models/nonsense/fetch")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (_, info) = send(
            app(dir.path()),
            authed("GET", "/api/info").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(info["desktop"], false);
    }

    fn json(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
        authed(method, uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn a_policy_is_tried_on_a_sample_and_a_bad_one_refused() {
        let dir = tempfile::tempdir().unwrap();
        let policy = serde_json::json!({
            "version": 1,
            "patterns": [{ "name": "customer-no", "entity": "CUSTOMER_NUMBER", "regex": r"KD-\d{6}" }],
            "allow": { "values": ["Muster GmbH"] }
        });
        let (status, body) = send(
            app(dir.path()),
            json("POST", "/api/policy/try", serde_json::json!({ "policy": policy, "sample": "Kunde KD-482913 der Muster GmbH" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["output"], "Kunde <CUSTOMER_NUMBER> der Muster GmbH");
        assert!(body["toml"].as_str().unwrap().contains("customer-no"));
        let bad = serde_json::json!({ "version": 1, "patterns": [{ "name": "x", "entity": "X", "regex": "(" }] });
        let (status, _) = send(
            app(dir.path()),
            json(
                "POST",
                "/api/policy/try",
                serde_json::json!({ "policy": bad }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn the_rules_offer_purposes_and_a_purpose_is_tried() {
        let dir = tempfile::tempdir().unwrap();
        let (status, body) = send(
            app(dir.path()),
            authed("GET", "/api/policy").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let names: Vec<&str> = body["purposes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["general", "legal", "development"]);
        assert!(
            body["entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e == "SECRET")
        );
        // A policy for development finds a key in the sample.
        let policy = serde_json::json!({ "version": 1, "purposes": ["development"] });
        let (status, body) = send(
            app(dir.path()),
            json(
                "POST",
                "/api/policy/try",
                serde_json::json!({ "policy": policy, "sample": "key AKIAIOSFODNN7EXAMPLE" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["output"], "key <SECRET>");
        // An unknown purpose is refused.
        let bad = serde_json::json!({ "version": 1, "purposes": ["medical"] });
        let (status, _) = send(
            app(dir.path()),
            json(
                "POST",
                "/api/policy/try",
                serde_json::json!({ "policy": bad }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn a_folder_run_writes_beside_and_lists_what_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "Mail an mira.albers@example.com").unwrap();
        std::fs::write(
            src.join("sub/b.csv"),
            "name,tel\nMira Albers,0421 5550173\n",
        )
        .unwrap();
        std::fs::write(src.join("c.bin"), [0u8, 159, 146, 150]).unwrap();
        let target = dir.path().join("out");
        let app = app(dir.path());
        // Not inside the source.
        let (status, _) = send(
            app.clone(),
            json("POST", "/api/batch", serde_json::json!({ "source": "in", "target": src.join("x").display().to_string() })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, body) = send(
            app.clone(),
            json(
                "POST",
                "/api/batch",
                serde_json::json!({ "source": "in", "target": target.display().to_string() }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 3);
        let done = loop {
            let (_, b) = send(
                app.clone(),
                authed("GET", "/api/batch").body(Body::empty()).unwrap(),
            )
            .await;
            if b["running"] == false {
                break b;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        assert_eq!(done["done"], 3);
        let a = std::fs::read_to_string(target.join("a.txt")).unwrap();
        assert_eq!(a, "Mail an <EMAIL_ADDRESS>");
        assert!(target.join("sub/b.csv").is_file());
        // Fail closed: what veilq cannot read is not copied.
        assert!(!target.join("c.bin").exists());
        let failed: Vec<_> = done["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["error"].is_string())
            .collect();
        assert_eq!(failed.len(), 1, "{done}");
        assert_eq!(
            failed[0]["error"], "unsupported format .bin",
            "the path is in the line"
        );
        assert!(target.join("veilq-report.json").is_file());
        // The target is not empty now: a second run there is refused.
        let (status, _) = send(
            app,
            json(
                "POST",
                "/api/batch",
                serde_json::json!({ "source": "in", "target": target.display().to_string() }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn folders_are_listed_by_name_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Akten")).unwrap();
        std::fs::write(dir.path().join("brief.txt"), "x").unwrap();
        let uri = format!(
            "/api/folders?path={}",
            dir.path()
                .display()
                .to_string()
                .replace('\\', "%5C")
                .replace(':', "%3A")
        );
        let (status, body) = send(
            app(dir.path()),
            authed("GET", &uri).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let names: Vec<&str> = body["folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Akten"]);
    }

    #[tokio::test]
    async fn a_picture_is_shown_before_it_is_redacted() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.png"), b"\x89PNG").unwrap();
        std::fs::write(root.join("b.txt"), "Hallo").unwrap();
        std::fs::write(outer.path().join("c.png"), b"\x89PNG").unwrap();
        let get = |path: &str| {
            authed("GET", &format!("/api/picture?path={path}"))
                .body(Body::empty())
                .unwrap()
        };
        let (status, body) = send(app(&root), get("a.png")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["media_type"], "image/png");
        assert_eq!(body["pages"], serde_json::json!(["iVBORw=="]));
        // Only images, and only under the root.
        let (status, _) = send(app(&root), get("b.txt")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) = send(app(&root), get("../c.png")).await;
        assert!(status.is_client_error(), "{status}");
        let (status, _) = send(
            app(&root),
            request("GET", "/api/picture?path=a.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn text_is_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let body = redact_body(
            serde_json::json!({ "kind": "text", "text": "Hallo Erika Mustermann, Tel. 0421 5550173" }),
        );
        let (status, body) = send(
            app(dir.path()),
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["output"], "Hallo <PERSON>, Tel. <PHONE_NUMBER>");
        assert_eq!(body["input_spans"][1], serde_json::json!([29, 41]));
        assert_eq!(body["output_spans"][0], serde_json::json!([6, 14]));
        assert_eq!(body["report"]["findings"][0]["entity"], "PERSON");
        assert!(body.get("explain").is_none(), "only when asked for");
    }

    #[tokio::test]
    async fn the_explanation_comes_only_when_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let body = serde_json::json!({
            "source": { "kind": "text", "text": "Hallo Erika Mustermann, Tel. 0421 5550173" },
            "explain": true,
        });
        let (status, body) = send(
            app(dir.path()),
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["output"], "Hallo <PERSON>, Tel. <PHONE_NUMBER>");
        let candidates = body["explain"][0]["candidates"].as_array().unwrap();
        assert!(
            candidates
                .iter()
                .any(|c| c["value"] == "0421 5550173" && c["verdict"] == "kept"),
            "{candidates:?}"
        );
    }

    #[tokio::test]
    async fn a_file_is_read_in_its_format() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("k.csv"),
            "name,geburtsdatum
Mira Albers,14.03.1987
",
        )
        .unwrap();
        std::fs::write(dir.path().join("k.docx"), "PK").unwrap();
        let req = |path: &str| {
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(redact_body(
                    serde_json::json!({ "kind": "file", "path": path }),
                ))
                .unwrap()
        };
        let (status, body) = send(app(dir.path()), req("k.csv")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["output"],
            "name,geburtsdatum
<PERSON>,<DATE_OF_BIRTH>
"
        );
        let (status, body) = send(app(dir.path()), req("k.docx")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("not an Office file"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn binary_input_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe, 0x00]);
        let body =
            redact_body(serde_json::json!({ "kind": "upload", "name": "x.txt", "data": data }));
        let (status, body) = send(
            app(dir.path()),
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["error"].as_str().unwrap().contains("not UTF-8"));
    }

    #[tokio::test]
    async fn a_proxy_in_front_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let proxied = Request::builder()
            .uri("/api/info")
            .header(header::HOST, format!("127.0.0.1:{PROXY_PORT}"))
            .header(header::ORIGIN, format!("http://localhost:{PROXY_PORT}"))
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(app(dir.path()), proxied).await.0, StatusCode::OK);
    }

    #[test]
    fn origins_stay_on_loopback() {
        assert_eq!(parse_origin("http://127.0.0.1:4760"), Ok(4760));
        assert_eq!(parse_origin("http://localhost:4760/"), Ok(4760));
        for bad in [
            "https://127.0.0.1:4760",
            "http://0.0.0.0:4760",
            "http://example.org:4760",
            "http://127.0.0.1:4760.evil",
            "http://127.0.0.1:0",
            "http://127.0.0.1",
        ] {
            assert!(parse_origin(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_token_file_keeps_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/ui-token");
        let first = token(Some(&file)).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(token(Some(&file)).unwrap(), first);
        std::fs::write(&file, "short").unwrap();
        assert!(token(Some(&file)).is_err());
        assert_ne!(token(None).unwrap(), first);
    }

    #[test]
    fn tokens_are_long_and_differ() {
        let a = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token().unwrap());
        assert!(constant_time_eq(a.as_bytes(), a.as_bytes()));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
