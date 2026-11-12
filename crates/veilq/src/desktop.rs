//! veilq as a desktop program (ADR-0027): started by a double click, it
//! opens the web UI in an app window; the setup page installs it for the
//! user, with a Start menu entry. Nothing here needs administrator rights.
//!
//! Windows is built; on macOS and Linux a double click is not told apart
//! from a shell yet, and setup reports itself as not available.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(windows)]
pub mod window;
#[cfg(windows)]
mod windows;

/// Whether veilq was started by a double click (or a shortcut) rather than
/// from a shell: on Windows, the console was made for this process alone.
pub fn double_clicked() -> bool {
    #[cfg(windows)]
    return windows::console_processes() <= 1;
    #[cfg(not(windows))]
    false
}

/// For a double click: false when the start went on here (no console
/// window was made, the manifest's policy, ADR-0031); true when this one
/// has a console window of its own and has started veilq again without
/// one, so this process should end.
pub fn handed_on() -> bool {
    #[cfg(windows)]
    return windows::console_processes() == 1
        && windows::relaunch_without_console(&["desktop"]).is_ok();
    #[cfg(not(windows))]
    false
}

/// Says why a double-click veilq did not start: in a message box on
/// Windows, where it has no console; on the console elsewhere.
pub fn report_error(message: &str) {
    #[cfg(windows)]
    windows::error_box(message);
    #[cfg(not(windows))]
    eprintln!("{message}");
}

/// veilq's own directory for the user: the token, the running instance.
pub fn app_dir() -> PathBuf {
    let base = if cfg!(windows) {
        // In an MSIX package: the package's own folder (ADR-0039).
        crate::package::local_data().or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("veilq")
}

/// What a double-click veilq keeps between starts: the folder it reads.
#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
}

impl Settings {
    fn file() -> PathBuf {
        app_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let file = Self::file();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(file, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// The UI a double click started, so that the next one opens it instead of
/// starting a second server.
#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub url: String,
    pub port: u16,
    pub pid: u32,
}

pub fn instance_file() -> PathBuf {
    app_dir().join("ui.json")
}

/// The running instance, if its file names one that answers.
pub fn running_instance() -> Option<Instance> {
    let text = std::fs::read_to_string(instance_file()).ok()?;
    let instance: Instance = serde_json::from_str(&text).ok()?;
    healthy(instance.port).then_some(instance)
}

/// Whether a veilq UI answers `/healthz` on `port`.
fn healthy(port: u16) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(500),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let request =
        format!("GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if s.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut body = String::new();
    let _ = s.read_to_string(&mut body);
    body.starts_with("HTTP/1.1 200") && body.ends_with("ok")
}

/// Asks the running instance to bring its own window to the front; false
/// when it has none (a browser window) or does not answer.
pub fn focus_running(instance: &Instance) -> bool {
    use std::io::{Read, Write};
    let Some(token) = instance.url.split("#token=").nth(1) else {
        return false;
    };
    #[cfg(windows)]
    windows::allow_foreground(instance.pid);
    let Ok(mut s) = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], instance.port)),
        Duration::from_millis(500),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let request = format!(
        "POST /api/focus HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        instance.port
    );
    if s.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    let _ = s.read_to_string(&mut answer);
    answer.starts_with("HTTP/1.1 204")
}

/// Hides the console window a double click opened: the UI is the program.
pub fn hide_console() {
    #[cfg(windows)]
    windows::hide_console();
}

/// Opens `url` in a browser window without tabs and address bar (Edge or
/// Chrome with `--app`), else in the default browser.
pub fn open_app_window(url: &str) {
    for browser in app_browsers() {
        if browser.is_file()
            && std::process::Command::new(&browser)
                .arg(format!("--app={url}"))
                .spawn()
                .is_ok()
        {
            return;
        }
    }
    crate::ui::open_browser(url);
}

#[allow(unused_mut)] // Linux has none.
fn app_browsers() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            out.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
            out.push(base.join(r"Google\Chrome\Application\chrome.exe"));
        }
    }
    #[cfg(target_os = "macos")]
    for app in ["Google Chrome", "Microsoft Edge"] {
        out.push(PathBuf::from(format!(
            "/Applications/{app}.app/Contents/MacOS/{app}"
        )));
    }
    out
}

/// What the setup page shows.
#[derive(Serialize)]
pub struct Status {
    /// Whether this platform's setup is built.
    pub available: bool,
    /// The running program, and where setup puts it.
    pub exe: String,
    pub install_dir: String,
    /// Whether a copy is in the install directory.
    pub installed: bool,
    /// Whether the running program is that copy.
    pub runs_installed: bool,
    pub start_menu: bool,
    pub desktop: bool,
    pub on_path: bool,
}

/// What the setup page asks for.
#[derive(Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))] // Setup is built for Windows so far.
#[serde(deny_unknown_fields)]
pub struct Install {
    pub start_menu: bool,
    pub desktop: bool,
    pub path: bool,
}

/// What uninstall removes besides the program: the models too, if asked.
#[derive(Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))] // Setup is built for Windows so far.
#[serde(deny_unknown_fields)]
pub struct Uninstall {
    #[serde(default)]
    pub models: bool,
}

/// Whether this veilq offers setup: on Windows, unless the Microsoft Store
/// installed it (ADR-0039).
pub fn setup_available() -> bool {
    cfg!(windows) && !crate::package::packaged()
}

pub fn status() -> Status {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = install_dir();
    #[cfg(windows)]
    let (start_menu, desktop, on_path) = (
        windows::start_menu_link().is_file(),
        windows::desktop_link().is_file(),
        windows::on_path(&dir),
    );
    #[cfg(not(windows))]
    let (start_menu, desktop, on_path) = (false, false, false);
    Status {
        available: setup_available(),
        installed: dir.join(exe_name()).is_file(),
        runs_installed: same_file(&exe, &dir.join(exe_name())),
        exe: plain(&exe),
        install_dir: plain(&dir),
        start_menu,
        desktop,
        on_path,
    }
}

/// Copies the program into the install directory (unless it runs from
/// there) and sets up what `request` asks for; what it does not ask for is
/// removed, so the page's switches are the state.
pub fn install(request: &Install) -> anyhow::Result<Status> {
    crate::package::refuse("installs")?;
    #[cfg(windows)]
    {
        let dir = install_dir();
        std::fs::create_dir_all(&dir)?;
        let target = dir.join(exe_name());
        let exe = std::env::current_exe()?;
        if !same_file(&exe, &target) {
            std::fs::copy(&exe, &target).map_err(|e| {
                anyhow::anyhow!(
                    "{}: {e} (is the installed veilq running?)",
                    target.display()
                )
            })?;
        }
        windows::set_link(&windows::start_menu_link(), request.start_menu, &target)?;
        windows::set_link(&windows::desktop_link(), request.desktop, &target)?;
        windows::set_path(&dir, request.path)?;
        windows::register_uninstall(&target)?;
        Ok(status())
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        anyhow::bail!("setup is built for Windows so far; see `veilq --help`")
    }
}

/// Removes shortcuts, the PATH entry, the uninstall entry and (after this
/// process ended) the installed program; the models too if asked.
pub fn uninstall(request: &Uninstall) -> anyhow::Result<()> {
    crate::package::refuse("uninstalls")?;
    #[cfg(windows)]
    {
        let dir = install_dir();
        windows::set_link(&windows::start_menu_link(), false, Path::new(""))?;
        windows::set_link(&windows::desktop_link(), false, Path::new(""))?;
        windows::set_path(&dir, false)?;
        windows::unregister_uninstall()?;
        if request.models {
            let models = veilq_core::models::model_dir();
            if models.is_dir() {
                std::fs::remove_dir_all(&models)?;
            }
        }
        let _ = std::fs::remove_file(instance_file());
        let _ = std::fs::remove_file(app_dir().join("ui-token"));
        windows::remove_after_exit(&dir)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        anyhow::bail!("setup is built for Windows so far")
    }
}

/// Where setup puts the program: `%LOCALAPPDATA%\Programs\veilq`.
pub fn install_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("Programs").join("veilq"))
            .unwrap_or_else(|| app_dir().join("bin"))
    } else {
        app_dir().join("bin")
    }
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "veilq.exe" } else { "veilq" }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// A path without Windows' `\\?\` prefix.
fn plain(p: &Path) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(s)
}
