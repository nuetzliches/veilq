//! Downloads a component's pinned files (ADR-0005) with `curl`, which every
//! supported system ships, and installs them after their SHA-256 matched.
//! Shared by `veilq models fetch` and the web UI's model page.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use veilq_core::models;

/// Where a download stands, for a progress display.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Progress {
    /// The file being fetched, and which of how many.
    pub file: String,
    pub index: usize,
    pub of: usize,
    /// Bytes of that file on disk so far.
    pub bytes: u64,
}

/// Whether `f` is installed in `dir` as this release pins it. A file taken
/// whole is hashed again: an older one at the same path (a pin that kept
/// its path, a restored cache) is fetched anew. A file taken from an archive
/// is pinned by the archive's hash and kept at a path with its version.
fn is_current(f: &models::PinnedFile, dir: &Path) -> bool {
    let path = dir.join(f.file);
    path.is_file() && (f.member.is_some() || models::sha256_of(&path).is_ok_and(|h| h == f.sha256))
}

/// Fetches and installs the files of `component` into `dir` that are not
/// there yet or not as this release pins them; `progress` is called as the
/// download grows.
pub fn component(
    component: &str,
    dir: &Path,
    progress: &(dyn Fn(&Progress) + Sync),
) -> anyhow::Result<()> {
    let files = models::pinned_for(component);
    if files.is_empty() {
        anyhow::bail!("this release pins no files for `{component}` on this platform");
    }
    let downloads = dir.join(".downloads");
    std::fs::create_dir_all(&downloads)?;
    for (i, f) in files.iter().enumerate() {
        if is_current(f, dir) {
            continue;
        }
        let tmp = downloads.join(f.name);
        let mut p = Progress {
            file: f.name.to_owned(),
            index: i + 1,
            of: files.len(),
            bytes: 0,
        };
        progress(&p);
        let mut child = Command::new("curl")
            .args(["-fsSL", "--proto", "=https", "--tlsv1.2", "-o"])
            .arg(&tmp)
            .arg(f.url())
            .spawn()
            .context("curl is needed to download (or use `veilq models install --from`)")?;
        // The file's size on disk is the progress; curl writes as it reads.
        let done = Arc::new(AtomicBool::new(false));
        let last = Mutex::new(0u64);
        let status = std::thread::scope(|s| {
            s.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    let size = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
                    if let Ok(mut l) = last.lock()
                        && *l != size
                    {
                        *l = size;
                        p.bytes = size;
                        progress(&p);
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
            });
            let status = child.wait();
            done.store(true, Ordering::Relaxed);
            status
        })?;
        if !status.success() {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("{}: download failed ({status})", f.url());
        }
        let result = models::install(f, &tmp, dir);
        let _ = std::fs::remove_file(&tmp);
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_at_its_path_counts_only_with_the_pinned_hash() {
        let dir = tempfile::tempdir().unwrap();
        let whole = models::pinned_for("face")
            .into_iter()
            .find(|f| f.member.is_none())
            .unwrap();
        assert!(!is_current(whole, dir.path()));
        // An older model at the same path is fetched anew.
        let path = dir.path().join(whole.file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"an older model").unwrap();
        assert!(!is_current(whole, dir.path()));
        // A file from an archive is pinned by the archive and kept by its
        // path, which names its version.
        if let Some(member) = models::PINNED.iter().find(|f| f.member.is_some()) {
            let path = dir.path().join(member.file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"a runtime").unwrap();
            assert!(is_current(member, dir.path()));
        }
    }
}
