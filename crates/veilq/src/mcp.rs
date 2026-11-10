//! `veilq mcp`: the MCP server (ADR-0016, ADR-0017), over stdio or
//! Streamable HTTP.
//!
//! Four tools: `scan`, `redact_text`, `redact_file`, `list_detectors`. They
//! return what the CLI returns, the report (schema/report.schema.json) and the
//! redacted text, and nothing more: no original value, no mapping, no way to
//! put pseudonyms back, no file written. Files are read under one root, with
//! every link followed (ADR-0007). Over HTTP every request needs the bearer
//! token, names a loopback host unless the server was bound elsewhere on
//! purpose, and carries no browser `Origin`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use veilq_core::{Engine, Format, Policy, root};

/// The largest input a tool reads, as in the UI.
const MAX_INPUT: usize = 32 << 20;
/// The environment variable that holds the HTTP bearer token.
pub const TOKEN_ENV: &str = "VEILQ_MCP_TOKEN";
const MIN_TOKEN_LEN: usize = 32;

pub struct Options {
    pub root: PathBuf,
    pub http: Option<SocketAddr>,
    pub policy: Policy,
    pub engine: Engine,
}

struct Inner {
    /// Canonical.
    root: PathBuf,
    policy: Policy,
    engine: Engine,
}

#[derive(Clone)]
pub struct Server {
    inner: Arc<Inner>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextParams {
    /// The text to redact.
    pub text: String,
    /// `text` (default), `ndjson`, `json`, `csv` or `tsv`.
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileParams {
    /// A file under the server's root, relative to it or absolute.
    #[serde(default)]
    pub path: Option<String>,
    /// The file's content, base64, instead of a path (a file outside the root).
    #[serde(default)]
    pub content_base64: Option<String>,
    /// The content's file name: its extension picks the format.
    #[serde(default)]
    pub name: Option<String>,
    /// Overrides the format the extension names: `text`, `ndjson`, `json`,
    /// `csv`, `tsv`.
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScanParams {
    /// A text to scan, instead of a file.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(flatten)]
    pub file: FileParams,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

fn failure(message: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.to_string())])
}

fn format_named(name: Option<&str>) -> Result<Option<Format>, String> {
    name.map(|n| Format::from_name(n).map_err(|e| e.to_string()))
        .transpose()
}

impl Server {
    pub fn new(root: PathBuf, policy: Policy, engine: Engine) -> Self {
        Self {
            inner: Arc::new(Inner {
                root,
                policy,
                engine,
            }),
        }
    }

    /// The text of a file source and the format to read it in.
    fn read(&self, p: &FileParams) -> Result<(String, Vec<u8>, Format), String> {
        let (name, bytes) = match (&p.path, &p.content_base64) {
            (Some(path), None) => {
                let resolved = root::resolve_under(&self.inner.root, Path::new(path))
                    .map_err(|e| e.to_string())?;
                let size = resolved
                    .metadata()
                    .map_err(|e| format!("{path}: {e}"))?
                    .len();
                if size > MAX_INPUT as u64 {
                    return Err(format!(
                        "{path} is larger than {} MiB; use `veilq redact`",
                        MAX_INPUT >> 20
                    ));
                }
                (
                    path.clone(),
                    std::fs::read(&resolved).map_err(|e| format!("{path}: {e}"))?,
                )
            }
            (None, Some(data)) => {
                let name = p.name.clone().unwrap_or_else(|| "content.txt".into());
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|e| format!("{name}: {e}"))?;
                (name, bytes)
            }
            _ => return Err("give either `path` or `content_base64`".into()),
        };
        if bytes.len() > MAX_INPUT {
            return Err(format!("{name} is larger than {} MiB", MAX_INPUT >> 20));
        }
        let format = match format_named(p.format.as_deref())? {
            Some(f) => f,
            None => Format::from_path(Path::new(&name)).map_err(|e| e.to_string())?,
        };
        Ok((name, bytes, format))
    }

    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Inner) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || f(&inner))
            .await
            .map_err(|e| e.to_string())?
    }
}

/// A file scanned or redacted: text formats as text, Office files as base64.
fn file_result(
    s: &Inner,
    name: String,
    bytes: &[u8],
    format: Format,
    apply: bool,
) -> Result<serde_json::Value, String> {
    if format.is_container() {
        let (out, run) = s
            .engine
            .run_bytes(bytes, format, apply)
            .map_err(|e| e.to_string())?;
        let mut v = serde_json::json!({ "name": name, "report": run.report });
        if apply {
            v["output_base64"] = base64::engine::general_purpose::STANDARD.encode(out).into();
        }
        return Ok(v);
    }
    // Fail closed: what veilq cannot read is an error, not a pass-through.
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{name} is not UTF-8 text"))?;
    if apply {
        let r = s
            .engine
            .redact_str(text, format)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "name": name, "output": r.text, "report": r.report }))
    } else {
        let r = s.engine.scan_str(text, format).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "name": name, "report": r.report }))
    }
}

fn result(value: Result<serde_json::Value, String>) -> Result<CallToolResult, ErrorData> {
    Ok(match value {
        Ok(v) => CallToolResult::structured(v),
        Err(e) => failure(e),
    })
}

#[tool_router]
impl Server {
    #[tool(
        description = "Find personal data in a text or a file (under the server's root, or base64 content). Returns the report: per finding the type, where, the confidence and the detector; never the value. Formats: text, ndjson, json, csv, tsv, docx, xlsx, pptx."
    )]
    async fn scan(
        &self,
        Parameters(p): Parameters<ScanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let source = match p.text {
            Some(text) if p.file.path.is_none() && p.file.content_base64.is_none() => {
                format_named(p.file.format.as_deref())
                    .map(|f| (None::<String>, text, f.unwrap_or(Format::Text)))
            }
            Some(_) => Err("give either `text`, `path` or `content_base64`".into()),
            None => {
                return result(match self.read(&p.file) {
                    Err(e) => Err(e),
                    Ok((name, bytes, format)) => {
                        self.blocking(move |s| file_result(s, name, &bytes, format, false))
                            .await
                    }
                });
            }
        };
        result(match source {
            Err(e) => Err(e),
            Ok((name, text, format)) => {
                self.blocking(move |s| {
                    let r = s
                        .engine
                        .scan_str(&text, format)
                        .map_err(|e| e.to_string())?;
                    Ok(serde_json::json!({ "name": name, "report": r.report }))
                })
                .await
            }
        })
    }

    #[tool(
        description = "Redact personal data in a text. Returns `output` (the text with each finding replaced as the policy says: <TYPE>, [TYPE:hex8], a stand-in, or nothing) and the `report`. Formats: text (default), ndjson, json, csv, tsv."
    )]
    async fn redact_text(
        &self,
        Parameters(p): Parameters<TextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let format = match format_named(p.format.as_deref()) {
            Ok(f) => f.unwrap_or(Format::Text),
            Err(e) => return result(Err(e)),
        };
        let text = p.text;
        result(
            self.blocking(move |s| {
                let r = s
                    .engine
                    .redact_str(&text, format)
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({ "output": r.text, "report": r.report }))
            })
            .await,
        )
    }

    #[tool(
        description = "Redact personal data in a file: a path under the server's root, or base64 `content_base64` with a `name`. Returns `name`, the redacted content in the file's format (`output` for text, `output_base64` for .docx .xlsx .pptx .eml, images and PDF) and the `report`. Writes nothing. Formats by extension: .txt .log .md, .ndjson .jsonl, .json, .csv, .tsv, .docx, .xlsx, .pptx, .eml, .png .jpg .jpeg .webp .gif and .pdf (with OCR installed; pictures are redacted with black boxes, a PDF becomes images of its pages); anything else is an error."
    )]
    async fn redact_file(
        &self,
        Parameters(p): Parameters<FileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let source = self.read(&p);
        result(match source {
            Err(e) => Err(e),
            Ok((name, bytes, format)) => {
                self.blocking(move |s| file_result(s, name, &bytes, format, true))
                    .await
            }
        })
    }

    #[tool(
        description = "The detectors this server runs: per rule its detector name, the entity type it finds and its locale; the policy's own patterns; the validators; the default action."
    )]
    async fn list_detectors(
        &self,
        Parameters(_): Parameters<NoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let policy = &self.inner.policy;
        let mut rules = Vec::new();
        for locale in std::iter::once("intl").chain(policy.locales.iter().map(String::as_str)) {
            for r in veilq_core::detect::pack(locale).unwrap_or_default() {
                rules.push(serde_json::json!({
                    "detector": format!("pattern:{locale}.{}", r.name),
                    "entity": r.entity,
                    "locale": locale,
                }));
            }
        }
        for r in &policy.patterns {
            rules.push(serde_json::json!({
                "detector": format!("policy:{}", r.name),
                "entity": r.entity,
                "locale": null,
            }));
        }
        result(Ok(serde_json::json!({
            "locales": policy.locales,
            "rules": rules,
            "validators": veilq_core::detect::validate::VALIDATORS,
            "default_action": policy.defaults.action.name(),
        })))
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "veilq finds and redacts personal data on this machine. Redact a file or text before you read, attach or send it: \
             redact_file for a path under the server's root, redact_text for text you have. scan reports what is there without \
             changing it. Results never contain the original values, and pseudonyms cannot be put back through this server.",
        )
    }
}

pub fn run(options: Options) -> anyhow::Result<()> {
    let root = options
        .root
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("root {}: {e}", options.root.display()))?;
    let server = Server::new(root, options.policy, options.engine);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match options.http {
        None => runtime.block_on(async {
            // stdout is the protocol: nothing else may be printed there.
            let running = server.serve(rmcp::transport::stdio()).await?;
            running.waiting().await?;
            Ok(())
        }),
        Some(addr) => runtime.block_on(serve_http(server, addr)),
    }
}

/// The token from [`TOKEN_ENV`], else a new one, printed once to stderr.
fn token() -> anyhow::Result<String> {
    match std::env::var(TOKEN_ENV) {
        Ok(t) if t.len() >= MIN_TOKEN_LEN => Ok(t),
        Ok(_) => anyhow::bail!("{TOKEN_ENV} must be at least {MIN_TOKEN_LEN} characters"),
        Err(_) => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes)
                .map_err(|e| anyhow::anyhow!("no randomness for the token: {e}"))?;
            let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("token:  {t}  (set {TOKEN_ENV} to choose one)");
            Ok(t)
        }
    }
}

pub fn router(server: Server, token: String, addr: SocketAddr) -> axum::Router {
    let mut config = StreamableHttpServerConfig::default()
        .enforce_origin_validation()
        .with_max_request_body_bytes(MAX_INPUT / 3 * 4 + (64 << 10));
    if !addr.ip().is_loopback() {
        // Bound elsewhere on purpose: the address it was bound to is a host
        // its clients may name, besides loopback.
        let mut hosts = config.allowed_hosts.clone();
        hosts.push(addr.ip().to_string());
        config = config.with_allowed_hosts(hosts);
    }
    let service: StreamableHttpService<Server, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(server.clone()), Default::default(), config);
    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            Arc::new(token),
            require_token,
        ))
}

async fn serve_http(server: Server, addr: SocketAddr) -> anyhow::Result<()> {
    let token = token()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    eprintln!("veilq mcp: http://{addr}/mcp  (Authorization: Bearer <token>)");
    eprintln!("root:   {}", display_path(&server.inner.root));
    axum::serve(listener, router(server, token, addr))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn require_token(State(token): State<Arc<String>>, request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let same = given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            == 0;
    if !same {
        return (StatusCode::UNAUTHORIZED, "missing or wrong bearer token").into_response();
    }
    next.run(request).await
}

fn display_path(path: &Path) -> String {
    let s = path.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt as _;

    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn server(root: &Path) -> Server {
        let policy = Policy::default();
        let engine = Engine::new(&policy, None, b"test", None).unwrap();
        Server::new(root.canonicalize().unwrap(), policy, engine)
    }

    fn structured(r: CallToolResult) -> serde_json::Value {
        assert_ne!(r.is_error, Some(true), "{r:?}");
        r.structured_content.expect("structured content")
    }

    fn error_text(r: CallToolResult) -> String {
        assert_eq!(r.is_error, Some(true), "{r:?}");
        serde_json::to_string(&r.content).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn redact_text_returns_output_and_report_only() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path());
        let r = s
            .redact_text(Parameters(TextParams {
                text: "Hallo Jonas Brandt, Tel. 0421 5550173".into(),
                format: None,
            }))
            .await
            .unwrap();
        let v = structured(r);
        assert_eq!(v["output"], "Hallo <PERSON>, Tel. <PHONE_NUMBER>");
        assert_eq!(v["report"]["findings"][1]["entity"], "PHONE_NUMBER");
        assert!(!v.to_string().contains("5550173"), "{v}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn files_are_read_under_the_root_only() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("k.csv"),
            "name,email\nMira Albers,m@example.org\n",
        )
        .unwrap();
        std::fs::write(outer.path().join("secret.txt"), "Frau Mira Albers").unwrap();
        let s = server(&root);
        let file = |path: &str| FileParams {
            path: Some(path.into()),
            content_base64: None,
            name: None,
            format: None,
        };
        let v = structured(s.redact_file(Parameters(file("k.csv"))).await.unwrap());
        assert_eq!(v["output"], "name,email\n<PERSON>,<EMAIL_ADDRESS>\n");
        let e = error_text(
            s.redact_file(Parameters(file("../secret.txt")))
                .await
                .unwrap(),
        );
        assert!(e.contains("outside the allowed root"), "{e}");
        assert!(!e.contains("Albers"));
        assert!(
            std::fs::read_dir(&root).unwrap().count() == 1,
            "nothing written"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn content_and_scan() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path());
        let data =
            base64::engine::general_purpose::STANDARD.encode("{\"email\":\"a.b@example.org\"}");
        let v = structured(
            s.scan(Parameters(ScanParams {
                text: None,
                file: FileParams {
                    path: None,
                    content_base64: Some(data),
                    name: Some("x.json".into()),
                    format: None,
                },
            }))
            .await
            .unwrap(),
        );
        assert_eq!(v["report"]["findings"][0]["entity"], "EMAIL_ADDRESS");
        assert!(v.get("output").is_none());
        let e = error_text(
            s.redact_file(Parameters(FileParams {
                path: None,
                content_base64: Some(
                    base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe]),
                ),
                name: Some("x.docx".into()),
                format: None,
            }))
            .await
            .unwrap(),
        );
        assert!(e.contains("not an Office file"), "{e}");
        let v = structured(s.list_detectors(Parameters(NoParams {})).await.unwrap());
        assert!(
            v["rules"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["detector"] == "pattern:de.tax-id")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_needs_the_token_and_no_browser_origin() {
        let dir = tempfile::tempdir().unwrap();
        let addr: SocketAddr = "127.0.0.1:4770".parse().unwrap();
        let app = router(server(dir.path()), TOKEN.into(), addr);
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;
        let req = |token: Option<&str>, origin: Option<&str>| {
            let mut b = axum::http::Request::post("/mcp")
                .header(header::HOST, "127.0.0.1:4770")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream");
            if let Some(t) = token {
                b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
            }
            if let Some(o) = origin {
                b = b.header(header::ORIGIN, o);
            }
            b.body(Body::from(init)).unwrap()
        };
        let status = |r: Response| r.status();
        assert_eq!(
            status(app.clone().oneshot(req(None, None)).await.unwrap()),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(
                app.clone()
                    .oneshot(req(Some("wrong-token-wrong-token-wrong-tok"), None))
                    .await
                    .unwrap()
            ),
            StatusCode::UNAUTHORIZED
        );
        let ok = app.clone().oneshot(req(Some(TOKEN), None)).await.unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let body = to_bytes(ok.into_body(), usize::MAX).await.unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("veilq"),
            "{}",
            String::from_utf8_lossy(&body)
        );
        let from_page = app
            .clone()
            .oneshot(req(Some(TOKEN), Some("https://evil.example")))
            .await
            .unwrap();
        assert!(
            from_page.status().is_client_error(),
            "{}",
            from_page.status()
        );
        let rebound = axum::http::Request::post("/mcp")
            .header(header::HOST, "evil.example:4770")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from(init))
            .unwrap();
        assert!(
            app.oneshot(rebound)
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
    }
}
