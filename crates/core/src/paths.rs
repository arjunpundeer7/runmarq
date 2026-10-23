//! Application-owned file locations.
//!
//! `RUNMARQ_HOME` overrides the data directory (used by tests and portable
//! setups). Otherwise the platform's per-user local data directory is used:
//! `%LOCALAPPDATA%\runmarq` on Windows, `~/Library/Application Support/runmarq`
//! on macOS and `$XDG_DATA_HOME/runmarq` on Linux.
//!
//! Installs made before the rename live in a `Runmark` folder. Until they are
//! migrated, `resolve` keeps returning that folder, so every process agrees on
//! one location and no data is split between two.

use crate::brand::{brand, LEGACY_DATA_DIR_NAME};
use std::path::PathBuf;

pub const HOME_ENV: &str = "RUNMARQ_HOME";

/// Database file name used before the rename.
pub const LEGACY_DATABASE_FILE: &str = "runmark.db";

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

fn local_data_base() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir)
}

impl Paths {
    pub fn resolve() -> Paths {
        if let Some(p) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Paths {
                root: PathBuf::from(p),
            };
        }
        let current = Paths::standard();
        if !current.root.exists() {
            let legacy = Paths::legacy();
            if legacy.root.is_dir() {
                return legacy;
            }
        }
        current
    }

    /// Where the data directory belongs under the current name.
    pub fn standard() -> Paths {
        Paths::at(local_data_base().join(&brand().data_dir_name))
    }

    /// Where an install made before the rename keeps its data.
    pub fn legacy() -> Paths {
        Paths::at(local_data_base().join(LEGACY_DATA_DIR_NAME))
    }

    /// True when this is the pre-rename data directory.
    pub fn is_legacy(&self) -> bool {
        self.root.file_name().and_then(|n| n.to_str()) == Some(LEGACY_DATA_DIR_NAME)
    }

    pub fn at(root: impl Into<PathBuf>) -> Paths {
        Paths { root: root.into() }
    }

    /// Discovery file: port, token and pid of the running collector.
    pub fn discovery(&self) -> PathBuf {
        self.root.join("collector.json")
    }
    pub fn lock(&self) -> PathBuf {
        self.root.join("collector.lock")
    }
    /// The database, under its pre-rename file name if that is the only one present.
    pub fn database(&self) -> PathBuf {
        let db = self.root.join("runmarq.db");
        let legacy = self.root.join(LEGACY_DATABASE_FILE);
        if !db.exists() && legacy.exists() {
            legacy
        } else {
            db
        }
    }
    pub fn spool_dir(&self) -> PathBuf {
        self.root.join("spool")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// Per-install secrets (fingerprint salt). Never leaves the machine.
    pub fn install_file(&self) -> PathBuf {
        self.root.join("install.json")
    }
    /// Small preference snapshot read by the hook process, so `emit` never
    /// has to open the database.
    pub fn hook_config(&self) -> PathBuf {
        self.root.join("hook-config.json")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_falls_back_to_the_pre_rename_file() {
        let dir = std::env::temp_dir().join(format!("runmarq-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = Paths::at(&dir);
        assert_eq!(p.database(), dir.join("runmarq.db"));
        std::fs::write(dir.join(LEGACY_DATABASE_FILE), b"").unwrap();
        assert_eq!(p.database(), dir.join(LEGACY_DATABASE_FILE));
        std::fs::write(dir.join("runmarq.db"), b"").unwrap();
        assert_eq!(p.database(), dir.join("runmarq.db"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recognises_the_pre_rename_folder() {
        assert!(Paths::legacy().is_legacy());
        assert!(!Paths::standard().is_legacy());
        assert!(!Paths::at("/tmp/elsewhere").is_legacy());
    }
}
