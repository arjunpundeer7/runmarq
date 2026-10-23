//! Single-instance locking, credentials and discovery.
//!
//! The collector binds to 127.0.0.1 on an ephemeral port and writes the port
//! plus a random bearer token to `collector.json` in the data directory.
//!
//! Protection of that file: on Unix the data directory is created `0700` and
//! the file `0600`. On Windows the directory lives under `%LOCALAPPDATA%`,
//! whose inherited ACL grants access only to the user, SYSTEM and
//! Administrators; runmarq does not widen it.

use rand::RngCore;
use runmarq_core::api::Discovery;
use runmarq_core::paths::Paths;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("another collector is already running (pid {0:?})")]
    AlreadyRunning(Option<u32>),
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
}

fn io(p: &Path) -> impl FnOnce(std::io::Error) -> LifecycleError + '_ {
    move |e| LifecycleError::Io(p.display().to_string(), e)
}

pub fn ensure_dirs(paths: &Paths) -> Result<(), LifecycleError> {
    for d in [
        &paths.root,
        &paths.spool_dir(),
        &paths.logs_dir(),
        &paths.backups_dir(),
    ] {
        std::fs::create_dir_all(d).map_err(io(d))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).map_err(io(d))?;
        }
    }
    Ok(())
}

/// Creates (or truncates) a file readable only by the current user.
pub fn private_file(path: &Path, create_new: bool) -> std::io::Result<File> {
    let mut o = OpenOptions::new();
    o.write(true);
    if create_new {
        o.create_new(true);
    } else {
        o.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
        o.custom_flags(libc_nofollow());
    }
    o.open(path)
}

#[cfg(unix)]
fn libc_nofollow() -> i32 {
    // O_NOFOLLOW: refuse to open through a symlink planted at the target path.
    #[cfg(target_os = "linux")]
    {
        0o400000
    }
    #[cfg(target_os = "macos")]
    {
        0x0100
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

/// Holds the exclusive instance lock for the life of the process.
pub struct InstanceLock {
    _file: File,
}

pub fn acquire_lock(paths: &Paths) -> Result<InstanceLock, LifecycleError> {
    use fs4::fs_std::FileExt;
    let p = paths.lock();
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&p)
        .map_err(io(&p))?;
    match file.try_lock_exclusive() {
        Ok(true) => Ok(InstanceLock { _file: file }),
        Ok(false) => Err(LifecycleError::AlreadyRunning(
            read_discovery(paths).map(|d| d.pid),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Err(
            LifecycleError::AlreadyRunning(read_discovery(paths).map(|d| d.pid)),
        ),
        Err(e) => Err(LifecycleError::Io(p.display().to_string(), e)),
    }
}

/// True if a collector currently holds the instance lock.
pub fn is_locked(paths: &Paths) -> bool {
    use fs4::fs_std::FileExt;
    let Ok(file) = OpenOptions::new().read(true).write(true).open(paths.lock()) else {
        return false;
    };
    match file.try_lock_exclusive() {
        Ok(true) => {
            let _ = FileExt::unlock(&file);
            false
        }
        _ => true,
    }
}

pub fn generate_token() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

pub fn write_discovery(paths: &Paths, d: &Discovery) -> Result<(), LifecycleError> {
    let target = paths.discovery();
    let tmp = target.with_extension(format!("json.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut f = private_file(&tmp, true).map_err(io(&tmp))?;
    f.write_all(&serde_json::to_vec_pretty(d).expect("serializable"))
        .map_err(io(&tmp))?;
    f.sync_all().map_err(io(&tmp))?;
    drop(f);
    std::fs::rename(&tmp, &target).map_err(io(&target))
}

pub fn read_discovery(paths: &Paths) -> Option<Discovery> {
    let p = paths.discovery();
    let meta = std::fs::symlink_metadata(&p).ok()?;
    if !meta.is_file() || meta.len() > 4096 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(&p).ok()?).ok()
}

pub fn remove_discovery(paths: &Paths, pid: u32) {
    if read_discovery(paths).is_some_and(|d| d.pid == pid) {
        let _ = std::fs::remove_file(paths.discovery());
    }
}

/// Per-install random salt used to hash repository paths and prompts.
pub fn install_salt(paths: &Paths) -> Vec<u8> {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Install {
        salt: String,
    }
    let p = paths.install_file();
    if let Ok(bytes) = std::fs::read(&p) {
        if let Ok(i) = serde_json::from_slice::<Install>(&bytes) {
            if let Ok(s) = hex::decode(&i.salt) {
                if s.len() >= 16 {
                    return s;
                }
            }
        }
    }
    let mut salt = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let _ = std::fs::create_dir_all(&paths.root);
    if let Ok(mut f) = private_file(&p, true) {
        let _ = f.write_all(
            &serde_json::to_vec(&Install {
                salt: hex::encode(salt),
            })
            .expect("serializable"),
        );
        return salt.to_vec();
    }
    // Lost a creation race with another process: use whatever it wrote.
    std::fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice::<Install>(&b).ok())
        .and_then(|i| hex::decode(i.salt).ok())
        .unwrap_or_else(|| salt.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        ensure_dirs(&paths).unwrap();
        let lock = acquire_lock(&paths).unwrap();
        assert!(is_locked(&paths));
        assert!(matches!(
            acquire_lock(&paths),
            Err(LifecycleError::AlreadyRunning(_))
        ));
        drop(lock);
        assert!(!is_locked(&paths));
        assert!(acquire_lock(&paths).is_ok());
    }

    #[test]
    fn discovery_roundtrip_and_salt_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        ensure_dirs(&paths).unwrap();
        let d = Discovery {
            pid: 42,
            port: 1234,
            token: generate_token(),
            version: "x".into(),
            started_at: chrono::Utc::now(),
        };
        write_discovery(&paths, &d).unwrap();
        assert_eq!(read_discovery(&paths), Some(d));
        assert_eq!(generate_token().len(), 64);
        let a = install_salt(&paths);
        assert_eq!(a, install_salt(&paths));
        remove_discovery(&paths, 1);
        assert!(
            read_discovery(&paths).is_some(),
            "only the owner removes discovery"
        );
        remove_discovery(&paths, 42);
        assert!(read_discovery(&paths).is_none());
    }
}
