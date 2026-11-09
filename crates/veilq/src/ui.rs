//! `veilq ui`: a local web UI that sets the input beside the output
//! (ADR-0008).
//!
//! The server listens on loopback only. Every API request carries a token the
//! server made at start and handed to the browser in the URL's fragment, and
//! every request names this server in `Host` (and `Origin`, when sent): a web
//! page in the same browser, or one that rebinds its DNS name to 127.0.0.1,
//! gets nothing. Files are read under one root, as the MCP server reads them
//! (ADR-0007). The HTTP API is internal to the embedded UI, not a contract.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use veilq_core::{Error, Policy, Report, root};

static UI: Dir<'_> = include_dir!("$OUT_DIR/ui");

/// The largest input the UI redacts. Text formats only until M3; a log
/// export above this belongs to `veilq redact`, which streams.
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
    pub open: bool,
    pub policy: Policy,
    pub policy_source: String,
}

struct AppState {
    token: String,
    hosts: Vec<String>,
    origins: Vec<String>,
    /// Canonical.
    root: PathBuf,
    policy: Policy,
    policy_source: String,
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
    let state = Arc::new(AppState::new(
        new_token()?,
        &ports,
        root,
        options.policy,
        options.policy_source,
    ));

    let shown = options.proxy_port.unwrap_or(port);
    let url = format!("http://127.0.0.1:{shown}/#token={}", state.token);
    println!("veilq ui: {url}");
    println!("root:     {}", display_path(&state.root));
    println!("policy:   {}", state.policy_source);
    println!("Ctrl+C stops it.");
    if options.open {
        open_browser(&url);
    }

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
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
    ) -> Self {
        let hosts: Vec<String> = ports
            .iter()
            .flat_map(|p| [format!("127.0.0.1:{p}"), format!("localhost:{p}")])
            .collect();
        Self {
            token,
            origins: hosts.iter().map(|h| format!("http://{h}")).collect(),
            hosts,
            root,
            policy,
            policy_source,
        }
    }
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
    max_input: usize,
}

async fn info(State(state): State<Arc<AppState>>) -> axum::Json<Info> {
    axum::Json(Info {
        version: env!("CARGO_PKG_VERSION"),
        root: display_path(&state.root),
        policy: state.policy_source.clone(),
        default_action: state.policy.defaults.action.name(),
        max_input: MAX_INPUT,
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
    match tokio::task::spawn_blocking(move || list(&state.root, &query.dir)).await {
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
#[serde(deny_unknown_fields)]
struct RedactRequest {
    source: Source,
}

#[derive(Serialize)]
struct RedactResponse {
    /// The source's name: a file's path or upload name; absent for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    input: String,
    output: String,
    /// Per finding, the byte range of its replacement in `output`.
    output_spans: Vec<[usize; 2]>,
    report: Report,
}

async fn redact(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<RedactRequest>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || redact_source(&state, request.source)).await;
    match result {
        Ok(Ok(response)) => axum::Json(response).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn redact_source(state: &AppState, source: Source) -> Result<RedactResponse, Failure> {
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
                root::resolve_under(&state.root, Path::new(&path)).map_err(Failure::from)?;
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
    // Fail closed: what veilq cannot read is an error, not a pass-through.
    let input = String::from_utf8(bytes).map_err(|_| {
        Failure(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("{label} is not UTF-8 text; documents and images come in M3 and M4"),
        )
    })?;
    // The key for `pseudonymize` comes from the environment the policy names.
    let key = match &state.policy.pseudonymize {
        Some(p) => match std::env::var(&p.key_env) {
            Ok(k) => Some(veilq_core::PseudonymKey::new(k.into_bytes()).map_err(Failure::from)?),
            Err(_) => None,
        },
        None => None,
    };
    let redacted =
        veilq_core::redact_text(&input, &state.policy, key.as_ref()).map_err(Failure::from)?;
    Ok(RedactResponse {
        name,
        input,
        output: redacted.text,
        output_spans: redacted
            .output_spans
            .iter()
            .map(|r| [r.start, r.end])
            .collect(),
        report: redacted.report,
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

fn open_browser(url: &str) {
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
        )))
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
    async fn redaction_fails_closed_until_m1() {
        let dir = tempfile::tempdir().unwrap();
        let body = redact_body(serde_json::json!({ "kind": "text", "text": "Erika Mustermann" }));
        let (status, body) = send(
            app(dir.path()),
            authed("POST", "/api/redact")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert!(body["error"].as_str().unwrap().contains("not built yet"));
        assert!(body.get("output").is_none());
    }

    #[tokio::test]
    async fn binary_input_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe, 0x00]);
        let body =
            redact_body(serde_json::json!({ "kind": "upload", "name": "x.bin", "data": data }));
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
    fn tokens_are_long_and_differ() {
        let a = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token().unwrap());
        assert!(constant_time_eq(a.as_bytes(), a.as_bytes()));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
