//! Safe editing of vendor JSON configuration files.
//!
//! Rules:
//! - Never modify a file that does not parse; report it instead.
//! - Back up the original before every change.
//! - Write atomically (temp file in the same directory, then rename).
//! - Follow symlinks to their target so dotfile setups keep working.
//! - Preserve key order and every key runmarq does not own.

use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("refusing to modify {path}: it is not valid JSON ({reason}). Fix or move the file, then retry.")]
    Invalid { path: PathBuf, reason: String },
    #[error("refusing to modify {path}: expected a JSON object at the top level")]
    NotObject { path: PathBuf },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("refusing to modify {path}: the file is larger than 5 MB")]
    TooLarge { path: PathBuf },
}

pub type Result<T> = std::result::Result<T, ConfigError>;

const MAX_CONFIG_BYTES: u64 = 5 * 1024 * 1024;

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ConfigError + '_ {
    move |source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Resolves symlinks so writes land on the real file.
pub fn real_path(path: &Path) -> PathBuf {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

/// Reads a JSON object. Missing or empty files read as `None`.
pub fn read_object(path: &Path) -> Result<Option<serde_json::Map<String, Value>>> {
    let path = real_path(path);
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ConfigError::Io { path, source: e }),
    };
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge { path });
    }
    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(ConfigError::NotObject { path }),
        Err(e) => Err(ConfigError::Invalid {
            path,
            reason: e.to_string(),
        }),
    }
}

/// Copies `path` into `backups_dir` with a timestamped name. Returns `None`
/// when there is nothing to back up.
pub fn backup(path: &Path, backups_dir: &Path, label: &str) -> Result<Option<PathBuf>> {
    let path = real_path(path);
    if !path.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(backups_dir).map_err(io(backups_dir))?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let dest = backups_dir.join(format!("{label}-{stamp}-{name}"));
    std::fs::copy(&path, &dest).map_err(io(&dest))?;
    Ok(Some(dest))
}

/// Atomically replaces `path` with pretty-printed `value`.
pub fn write_object(path: &Path, value: &serde_json::Map<String, Value>) -> Result<()> {
    let path = real_path(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let mut text = serde_json::to_string_pretty(value).expect("JSON values always serialize");
    text.push('\n');
    let tmp = path.with_extension(format!("{}.runmarq-tmp", std::process::id()));
    std::fs::write(&tmp, text.as_bytes()).map_err(io(&tmp))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(&path) {
            let _ = std::fs::set_permissions(
                &tmp,
                std::fs::Permissions::from_mode(m.permissions().mode()),
            );
        }
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(ConfigError::Io { path, source: e });
    }
    Ok(())
}

/// Convenience: the `hooks` object of a config, created if missing.
pub fn hooks_object(
    root: &mut serde_json::Map<String, Value>,
) -> &mut serde_json::Map<String, Value> {
    if !root.get("hooks").is_some_and(Value::is_object) {
        root.insert("hooks".into(), Value::Object(Default::default()));
    }
    root.get_mut("hooks")
        .and_then(Value::as_object_mut)
        .expect("just inserted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn refuses_invalid_and_preserves_order() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(matches!(read_object(&p), Err(ConfigError::Invalid { .. })));
        std::fs::write(&p, "[1,2]").unwrap();
        assert!(matches!(
            read_object(&p),
            Err(ConfigError::NotObject { .. })
        ));

        std::fs::write(&p, r#"{"zeta": 1, "alpha": {"b": 2, "a": 1}}"#).unwrap();
        let mut obj = read_object(&p).unwrap().unwrap();
        obj.insert("hooks".into(), json!({}));
        let b = backup(&p, &dir.path().join("backups"), "claude")
            .unwrap()
            .unwrap();
        write_object(&p, &obj).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.find("zeta").unwrap() < text.find("alpha").unwrap(),
            "key order preserved"
        );
        assert_eq!(
            std::fs::read_to_string(b).unwrap(),
            r#"{"zeta": 1, "alpha": {"b": 2, "a": 1}}"#
        );
    }

    #[test]
    fn missing_and_empty_are_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.json");
        assert!(read_object(&p).unwrap().is_none());
        std::fs::write(&p, "\u{feff}  \n").unwrap();
        assert!(read_object(&p).unwrap().is_none());
        assert!(backup(&dir.path().join("missing.json"), dir.path(), "x")
            .unwrap()
            .is_none());
    }
}
