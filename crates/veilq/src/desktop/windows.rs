//! The Windows side of [`super`]: the console, shortcuts, the user's PATH
//! and the entry in "Installed apps". All of it is the user's own (HKCU,
//! the user's Start menu): no administrator rights.
//!
//! The five calls into the Windows API are the only `unsafe` code in
//! veilq (ADR-0027, ADR-0029, ADR-0031); each says why it is sound.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, RegType};

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\veilq";

/// How many processes share this process's console: 0 when it has none
/// (the manifest's `consoleAllocationPolicy` kept the Explorer from making
/// one, ADR-0031), 1 when the Explorer made one for it alone (a Windows
/// that does not know the policy), more when a shell shares its own.
pub fn console_processes() -> u32 {
    let mut ids = [0u32; 2];
    // SAFETY: `ids` is a valid, writable buffer of the length passed; the
    // call writes at most that many process ids and returns their count
    // (0 without a console).
    unsafe {
        windows_sys::Win32::System::Console::GetConsoleProcessList(
            ids.as_mut_ptr(),
            ids.len() as u32,
        )
    }
}

/// Starts this program again with `args`, without a console window: a
/// Windows that ignores the manifest gave the first start one, and Windows
/// Terminal keeps it open whatever is hidden.
pub fn relaunch_without_console(args: &[&str]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(std::env::current_exe()?)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// Shows `message` in a Windows message box: a veilq without a console has
/// nowhere else to say why it did not start.
pub fn error_box(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let text: Vec<u16> = message.encode_utf16().chain([0]).collect();
    let title: Vec<u16> = "veilq".encode_utf16().chain([0]).collect();
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call;
    // no owner window (null), so the box is modal to nothing.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Hides this process's console window, if it has one.
pub fn hide_console() {
    // SAFETY: GetConsoleWindow takes nothing and returns a window handle or
    // null; ShowWindow is given that handle only when it is not null.
    unsafe {
        let window = windows_sys::Win32::System::Console::GetConsoleWindow();
        if !window.is_null() {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                window,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
            );
        }
    }
}

/// Lets the running veilq (`pid`) take the foreground: a second double click
/// has that right, the running one in the background has not.
pub fn allow_foreground(pid: u32) {
    // SAFETY: a plain call with a process id; it fails (and nothing
    // happens) when this process may not hand the right on.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
    }
}

/// Tells running programs (the Explorer, new consoles) that the user's
/// environment changed, as the Settings app does after editing PATH.
fn broadcast_environment() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let what: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    // SAFETY: `what` is a NUL-terminated UTF-16 string that outlives the
    // call, `result` a valid place for the reply; the timeout bounds it.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            what.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            &mut result,
        );
    }
}

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

/// The user's Start menu entry.
pub fn start_menu_link() -> PathBuf {
    env_dir("APPDATA").join(r"Microsoft\Windows\Start Menu\Programs\veilq.lnk")
}

/// The user's desktop entry; the desktop may be moved (OneDrive), so its
/// place is read from the Explorer's settings.
pub fn desktop_link() -> PathBuf {
    let moved = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
        .and_then(|k| k.get_value::<String, _>("Desktop"))
        .ok()
        .map(|d| expand(&d));
    moved
        .map(PathBuf::from)
        .unwrap_or_else(|| env_dir("USERPROFILE").join("Desktop"))
        .join("veilq.lnk")
}

/// `%VAR%` replaced by the environment's value.
fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                out.push_str(&std::env::var(name).unwrap_or_else(|_| format!("%{name}%")));
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// A shortcut at `link` to `target` when `on`, else none.
pub fn set_link(link: &Path, on: bool, target: &Path) -> anyhow::Result<()> {
    if !on {
        if link.is_file() {
            std::fs::remove_file(link)?;
        }
        return Ok(());
    }
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut sl = mslnk::ShellLink::new(target)?;
    sl.set_name(Some("veilq: find and redact personal data".into()));
    sl.set_working_dir(target.parent().map(|d| d.display().to_string()));
    sl.set_icon_location(Some(target.display().to_string()));
    sl.create_lnk(link)?;
    Ok(())
}

fn environment(write: bool) -> std::io::Result<RegKey> {
    let access = if write {
        KEY_READ | KEY_WRITE
    } else {
        KEY_READ
    };
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", access)
}

/// The user's PATH as stored, with its type (`REG_EXPAND_SZ` keeps `%VAR%`).
fn user_path() -> (String, RegType) {
    match environment(false).and_then(|k| k.get_raw_value("Path")) {
        Ok(v) => {
            let units: Vec<u16> = v
                .bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .take_while(|u| *u != 0)
                .collect();
            (String::from_utf16_lossy(&units), v.vtype)
        }
        Err(_) => (String::new(), RegType::REG_EXPAND_SZ),
    }
}

fn same_dir(entry: &str, dir: &Path) -> bool {
    let a = expand(entry.trim().trim_end_matches('\\')).to_lowercase();
    let b = dir
        .display()
        .to_string()
        .trim_end_matches('\\')
        .to_lowercase();
    a == b
}

pub fn on_path(dir: &Path) -> bool {
    user_path().0.split(';').any(|e| same_dir(e, dir))
}

/// `dir` in the user's PATH when `on`, else not; other entries untouched.
pub fn set_path(dir: &Path, on: bool) -> anyhow::Result<()> {
    let (path, vtype) = user_path();
    let mut entries: Vec<&str> = path.split(';').filter(|e| !e.is_empty()).collect();
    let had = entries.iter().any(|e| same_dir(e, dir));
    if had == on {
        return Ok(());
    }
    let shown = dir.display().to_string();
    if on {
        entries.push(&shown);
    } else {
        entries.retain(|e| !same_dir(e, dir));
    }
    let joined = entries.join(";");
    let mut bytes: Vec<u8> = joined
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    if bytes.is_empty() {
        bytes = vec![0, 0];
    }
    environment(true)?.set_raw_value(
        "Path",
        &winreg::RegValue {
            bytes: bytes.into(),
            vtype,
        },
    )?;
    broadcast_environment();
    Ok(())
}

/// The entry in "Installed apps", whose uninstall runs `veilq uninstall`.
pub fn register_uninstall(exe: &Path) -> anyhow::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL)?;
    let exe_s = exe.display().to_string();
    key.set_value("DisplayName", &"veilq")?;
    key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    key.set_value("Publisher", &"veilq contributors")?;
    key.set_value("DisplayIcon", &exe_s)?;
    key.set_value(
        "InstallLocation",
        &exe.parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    )?;
    key.set_value("UninstallString", &format!("\"{exe_s}\" uninstall"))?;
    key.set_value("URLInfoAbout", &"https://github.com/nuetzliches/veilq")?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    Ok(())
}

pub fn unregister_uninstall() -> anyhow::Result<()> {
    match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Removes `dir` a few seconds after this process ended: a running program
/// cannot delete itself on Windows.
pub fn remove_after_exit(dir: &Path) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    if !dir.is_dir() {
        return Ok(());
    }
    std::process::Command::new("cmd")
        .arg("/c")
        .raw_arg(format!(
            "ping -n 4 127.0.0.1 >nul & rmdir /s /q \"{}\"",
            dir.display()
        ))
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_are_expanded() {
        let home = std::env::var("USERPROFILE").unwrap();
        assert_eq!(expand(r"%USERPROFILE%\Desktop"), format!(r"{home}\Desktop"));
        assert_eq!(expand("%VEILQ_NOT_SET_42%"), "%VEILQ_NOT_SET_42%");
        assert_eq!(expand("50% off"), "50% off");
    }

    #[test]
    fn a_path_entry_is_the_same_directory_whatever_its_case_and_slash() {
        let dir = Path::new(r"C:\Users\x\AppData\Local\Programs\veilq");
        assert!(same_dir(r"c:\users\x\appdata\local\programs\veilq\", dir));
        assert!(!same_dir(r"C:\Users\x\AppData\Local\Programs\veilq2", dir));
    }
}
