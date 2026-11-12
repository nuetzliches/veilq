//! veilq in an MSIX package, as the Microsoft Store installs it (ADR-0039).
//!
//! Windows installs a package itself, updates it from the Store and removes
//! it with everything it wrote. So in a package veilq neither installs
//! (setup), updates nor uninstalls itself. And Windows redirects what a
//! packaged program writes under `%LOCALAPPDATA%` into the package's own
//! folder, a view the programs it starts (`curl`, which fetches the models)
//! do not share: veilq names that folder itself, so every process writes
//! and reads the same files.
#![cfg_attr(windows, allow(unsafe_code))]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The package's family name when veilq runs from an MSIX package.
pub fn family_name() -> Option<&'static str> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(current_family_name).as_deref()
}

/// Whether veilq runs from an MSIX package.
pub fn packaged() -> bool {
    family_name().is_some()
}

/// What stands for `%LOCALAPPDATA%` in a package: the package's own copy of
/// it, where Windows puts what the package writes there anyway.
pub fn local_data() -> Option<PathBuf> {
    let family = family_name()?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(package_local(Path::new(&local), family))
}

fn package_local(local_app_data: &Path, family: &str) -> PathBuf {
    local_app_data
        .join("Packages")
        .join(family)
        .join("LocalCache")
        .join("Local")
}

/// Fails in a package: `what` is the Store's to do there.
pub fn refuse(what: &str) -> anyhow::Result<()> {
    if packaged() {
        anyhow::bail!(
            "this veilq is installed from the Microsoft Store, which {what} it (Settings → Apps)"
        );
    }
    Ok(())
}

#[cfg(windows)]
fn current_family_name() -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
    // PACKAGE_FAMILY_NAME_MAX_LENGTH is 64; with the NUL and room to spare.
    let mut name = [0u16; 128];
    let mut len = name.len() as u32;
    // SAFETY: `name` is a valid, writable buffer of `len` UTF-16 units; the
    // call writes at most that many (with the NUL) and sets `len`, or
    // returns an error (APPMODEL_ERROR_NO_PACKAGE outside a package).
    let result = unsafe { GetCurrentPackageFamilyName(&mut len, name.as_mut_ptr()) };
    if result != ERROR_SUCCESS {
        return None;
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..end])).filter(|n| !n.is_empty())
}

#[cfg(not(windows))]
fn current_family_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_folder_is_the_packages_local_cache() {
        assert_eq!(
            package_local(Path::new(r"C:\Users\a\AppData\Local"), "veilq_abc123"),
            Path::new(r"C:\Users\a\AppData\Local")
                .join("Packages")
                .join("veilq_abc123")
                .join("LocalCache")
                .join("Local")
        );
    }

    #[test]
    fn outside_a_package_nothing_is_refused() {
        // Tests never run from a package.
        assert!(!packaged());
        assert!(local_data().is_none());
        assert!(refuse("updates").is_ok());
    }
}
