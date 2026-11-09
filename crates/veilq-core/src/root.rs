//! The root a caller's paths must stay under (ADR-0007). The MCP server
//! resolves every path an agent names through [`resolve_under`]; without
//! it, an agent could make veilq read any file of the user and return it.

use std::path::{Path, PathBuf};

use crate::Error;

/// `path` (relative to `root`, or absolute) as a canonical path, if it exists
/// and lies under `root` after following every link. `..` and links that
/// lead out are refused.
pub fn resolve_under(root: &Path, path: &Path) -> Result<PathBuf, Error> {
    let root = root.canonicalize().map_err(|e| io(root, e))?;
    let joined = root.join(path);
    let resolved = joined.canonicalize().map_err(|e| io(path, e))?;
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(Error::OutsideRoot {
            path: path.to_path_buf(),
            root,
        })
    }
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_is_allowed_outside_is_not() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "a").unwrap();
        std::fs::write(outer.path().join("secret.txt"), "s").unwrap();

        assert!(resolve_under(&root, Path::new("sub/a.txt")).is_ok());
        assert!(resolve_under(&root, &root.join("sub/a.txt")).is_ok());
        assert!(matches!(
            resolve_under(&root, Path::new("../secret.txt")),
            Err(Error::OutsideRoot { .. })
        ));
        assert!(matches!(
            resolve_under(&root, &outer.path().join("secret.txt")),
            Err(Error::OutsideRoot { .. })
        ));
        assert!(matches!(
            resolve_under(&root, Path::new("missing.txt")),
            Err(Error::Io { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_root_is_refused() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(outer.path().join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(outer.path().join("secret.txt"), root.join("link")).unwrap();
        assert!(matches!(
            resolve_under(&root, Path::new("link")),
            Err(Error::OutsideRoot { .. })
        ));
    }
}
