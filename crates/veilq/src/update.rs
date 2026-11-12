//! The update check and update (ADR-0027, point 6): only when the user asks.
//! The check reads the newest release from GitHub's API and sends nothing
//! but the request; the update downloads this platform's archive of that
//! release, checks it against the release's `SHA256SUMS`, and puts the new
//! program where the running one is. The running one is renamed first: on
//! Windows a running program cannot be overwritten, but it can be moved.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use veilq_core::models;

const LATEST: &str = "https://api.github.com/repos/nuetzliches/veilq/releases/latest";
/// The target this program was built for, as the release names its archive.
const TARGET: &str = env!("VEILQ_TARGET");

/// What the check found.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub current: String,
    pub latest: String,
    /// Whether `latest` is newer than `current`.
    pub newer: bool,
    /// The release's page.
    pub url: String,
    /// This platform's archive in that release, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// `curl` with what every download here uses: HTTPS only, TLS 1.2 or newer.
fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-fsSL", "--proto", "=https", "--tlsv1.2"]);
    c
}

fn get(url: &str) -> anyhow::Result<Vec<u8>> {
    let out = curl()
        .args(["-H", "Accept: application/vnd.github+json", "-H"])
        .arg(format!("User-Agent: veilq/{}", env!("CARGO_PKG_VERSION")))
        .arg(url)
        .output()
        .context("curl is needed to check for updates")?;
    if !out.status.success() {
        anyhow::bail!(
            "{url}: {}",
            String::from_utf8_lossy(&out.stderr)
                .trim()
                .trim_start_matches("curl: ")
        );
    }
    Ok(out.stdout)
}

fn download(url: &str, to: &Path) -> anyhow::Result<()> {
    let status = curl()
        .arg("-o")
        .arg(to)
        .arg(url)
        .status()
        .context("curl is needed to update")?;
    if !status.success() {
        anyhow::bail!("{url}: download failed ({status})");
    }
    Ok(())
}

fn archive_name() -> String {
    let ext = if TARGET.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("veilq-{TARGET}.{ext}")
}

/// `1.2.3` from `v1.2.3`; anything that is not three numbers is no version.
fn version(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.trim_start_matches('v').splitn(3, '.');
    let mut next = || parts.next()?.split(['-', '+']).next()?.parse().ok();
    Some((next()?, next()?, next()?))
}

fn release() -> anyhow::Result<Release> {
    serde_json::from_slice(&get(LATEST)?).context("the release's answer could not be read")
}

pub fn check() -> anyhow::Result<Check> {
    Ok(check_of(&release()?))
}

fn check_of(r: &Release) -> Check {
    let current = env!("CARGO_PKG_VERSION").to_owned();
    let newer = match (version(&r.tag_name), version(&current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    };
    let name = archive_name();
    Check {
        latest: r.tag_name.trim_start_matches('v').to_owned(),
        current,
        newer,
        url: r.html_url.clone(),
        archive: r.assets.iter().any(|a| a.name == name).then_some(name),
    }
}

/// Downloads the newest release and puts it where the running program is;
/// returns the path of the new program, to start it. The old one stays as
/// `veilq.old(.exe)` until the next start removes it.
pub fn install_latest() -> anyhow::Result<(Check, PathBuf)> {
    crate::package::refuse("updates")?;
    let r = release()?;
    let found = check_of(&r);
    if !found.newer {
        anyhow::bail!("veilq {} is the newest release", found.current);
    }
    let name = archive_name();
    let url = |n: &str| {
        r.assets
            .iter()
            .find(|a| a.name == n)
            .map(|a| a.browser_download_url.clone())
            .ok_or_else(|| anyhow::anyhow!("release {} has no {n}", r.tag_name))
    };
    let (archive_url, sums_url) = (url(&name)?, url("SHA256SUMS")?);
    let sums = String::from_utf8(get(&sums_url)?)?;
    let expected = sums
        .lines()
        .filter_map(|l| l.split_once("  "))
        .find(|(_, n)| n.trim() == name)
        .map(|(h, _)| h.trim().to_lowercase())
        .ok_or_else(|| anyhow::anyhow!("SHA256SUMS of {} names no {name}", r.tag_name))?;

    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .context("the program has no folder")?
        .to_path_buf();
    let downloads = std::env::temp_dir().join(format!("veilq-update-{}", std::process::id()));
    std::fs::create_dir_all(&downloads)?;
    let archive = downloads.join(&name);
    let result = (|| -> anyhow::Result<PathBuf> {
        download(&archive_url, &archive)?;
        let got = models::sha256_of(&archive)?;
        if got != expected {
            anyhow::bail!(
                "{name}: SHA-256 {got}, the release says {expected}; nothing was changed"
            );
        }
        let program = if cfg!(windows) { "veilq.exe" } else { "veilq" };
        let fresh = downloads.join(program);
        if !models::unpack(&archive, &name, program, &fresh)? {
            anyhow::bail!("{name} has no {program}");
        }
        let target = dir.join(exe.file_name().context("the program has no name")?);
        let old = old_path(&target);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&target, &old)
            .with_context(|| format!("{}: could not move it aside", target.display()))?;
        if let Err(e) = std::fs::copy(&fresh, &target) {
            // Put the old one back: the program must stay where it was.
            let _ = std::fs::rename(&old, &target);
            return Err(e).with_context(|| format!("{}: could not write it", target.display()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(target)
    })();
    let _ = std::fs::remove_dir_all(&downloads);
    Ok((found, result?))
}

fn old_path(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("veilq");
    match exe.extension().and_then(|e| e.to_str()) {
        Some(ext) => exe.with_file_name(format!("{stem}.old.{ext}")),
        None => exe.with_file_name(format!("{stem}.old")),
    }
}

/// Removes what an update left: the program it replaced.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
}

/// Starts `program` as a double click would, once this process ended (a
/// second instance would only open this one's window).
#[cfg_attr(not(feature = "ui"), allow(dead_code))] // Only the UI restarts.
pub fn restart(program: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .arg("/c")
            .raw_arg(format!(
                "ping -n 3 127.0.0.1 >nul & start \"\" \"{}\"",
                program.display()
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
    }
    #[cfg(not(windows))]
    let _ = program;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(version("v0.1.10"), Some((0, 1, 10)));
        assert!(version("v0.1.10") > version("0.1.9"));
        assert_eq!(version("v1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(version("latest"), None);
    }

    #[test]
    fn the_archive_is_named_as_the_release_names_it() {
        let name = archive_name();
        assert!(
            name.starts_with("veilq-") && name.contains(TARGET),
            "{name}"
        );
        assert!(name.ends_with(".zip") || name.ends_with(".tar.gz"));
    }

    #[test]
    fn the_old_program_keeps_its_extension() {
        assert_eq!(
            old_path(Path::new("C:/p/veilq.exe")),
            Path::new("C:/p/veilq.old.exe")
        );
        assert_eq!(
            old_path(Path::new("/usr/bin/veilq")),
            Path::new("/usr/bin/veilq.old")
        );
    }
}
