//! Stable location for the hook executable.
//!
//! Hooks reference an absolute path. Pointing them at a build directory or
//! an app bundle breaks when either moves or updates, so setup copies the
//! `runmarq` executable to `<data dir>/bin/` and hooks use that copy. Setup
//! (and the desktop app on launch) refresh the copy when it differs.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn installed_path(data_root: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "runmarq.exe"
    } else {
        "runmarq"
    };
    data_root.join("bin").join(name)
}

/// The hook executable as it was named before the rename.
pub fn legacy_installed_path(data_root: &Path) -> PathBuf {
    let name = runmarq_core::brand::LEGACY_CLI_NAME;
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    data_root.join("bin").join(name)
}

fn digest(p: &Path) -> Option<[u8; 32]> {
    let bytes = std::fs::read(p).ok()?;
    Some(Sha256::digest(&bytes).into())
}

/// Copies `source` to the stable location if missing or different and
/// returns the stable path.
pub fn ensure_installed(source: &Path, data_root: &Path) -> std::io::Result<PathBuf> {
    let dest = installed_path(data_root);
    if dest == source {
        return Ok(dest);
    }
    if dest.is_file() && digest(&dest) == digest(source) {
        return Ok(dest);
    }
    let dir = dest.parent().expect("has parent");
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".runmarq-{}.tmp", std::process::id()));
    std::fs::copy(source, &tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    // On Windows a running executable cannot be overwritten, but it can be
    // renamed. Move the old copy aside first so a concurrently running hook
    // does not block the update.
    if dest.exists() && std::fs::rename(&tmp, &dest).is_err() {
        let old = dir.join(format!(
            ".runmarq-old-{}",
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::rename(&dest, &old)?;
        std::fs::rename(&tmp, &dest)?;
        let _ = std::fs::remove_file(&old);
    } else if !dest.exists() {
        std::fs::rename(&tmp, &dest)?;
    }
    // Clean up leftovers from earlier updates that were still running then.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(".runmarq-old-") || name.starts_with(".runmark-old-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    #[test]
    fn copies_and_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src-bin");
        std::fs::write(&src, b"v1").unwrap();
        let dest = super::ensure_installed(&src, dir.path()).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"v1");
        std::fs::write(&src, b"v2").unwrap();
        super::ensure_installed(&src, dir.path()).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"v2");
        assert_eq!(super::ensure_installed(&dest, dir.path()).unwrap(), dest);
    }
}
