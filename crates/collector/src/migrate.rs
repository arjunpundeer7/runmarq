//! One-time move of an install made before the product was renamed.
//!
//! Earlier versions kept their data in a `Runmark` folder and pointed agent
//! hooks at a `runmark` executable inside it. The first CLI command or app
//! launch that knows the new name moves that install over:
//!
//! 1. stops a collector still running from the old folder,
//! 2. renames the folder and the database files,
//! 3. installs the current hook executable and removes the old one,
//! 4. rewrites the hook entries of every agent and scope that had them, with
//!    a backup of each configuration file, exactly as setup does.
//!
//! The hook process never migrates: it runs on every agent event and must
//! stay fast. If any step before the rename fails, nothing has changed,
//! `Paths::resolve` keeps using the old folder, and the next start retries.

use crate::{lifecycle, Client};
use runmarq_adapters::{binary, InstallOptions, Scope};
use runmarq_core::paths::{Paths, LEGACY_DATABASE_FILE};
use runmarq_storage::Store;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Migration {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Agent configuration files whose hook entries were rewritten.
    pub rewritten: Vec<PathBuf>,
    /// Hook rewrites that failed. The data move itself succeeded.
    pub problems: Vec<String>,
}

/// Resolves the data directory, first migrating a pre-rename install when
/// one is found and `hook_executable` (the current `runmarq` CLI) is known.
pub fn resolve_migrating(hook_executable: Option<&Path>) -> (Paths, Option<Migration>) {
    let paths = Paths::resolve();
    let Some(exe) = hook_executable.filter(|_| paths.is_legacy()) else {
        return (paths, None);
    };
    match migrate(&paths, &Paths::standard(), exe, None) {
        Ok(m) => (Paths::at(&m.to), Some(m)),
        Err(e) => {
            tracing::warn!("could not move {} yet: {e}", paths.root.display());
            (paths, None)
        }
    }
}

pub fn migrate(
    old: &Paths,
    new: &Paths,
    exe: &Path,
    home: Option<PathBuf>,
) -> Result<Migration, String> {
    if new.root.exists() {
        return Err(format!("{} already exists", new.root.display()));
    }
    stop_collector(old)?;
    let scopes = recorded_scopes(old);

    std::fs::rename(&old.root, &new.root).map_err(|e| {
        format!(
            "renaming {} to {}: {e}",
            old.root.display(),
            new.root.display()
        )
    })?;

    let mut problems = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let from = new.root.join(format!("{LEGACY_DATABASE_FILE}{suffix}"));
        if from.exists() {
            let to = new.root.join(format!("runmarq.db{suffix}"));
            if let Err(e) = std::fs::rename(&from, &to) {
                problems.push(format!("renaming {}: {e}", from.display()));
            }
        }
    }

    let installed = match binary::ensure_installed(exe, &new.root) {
        Ok(p) => p,
        Err(e) => {
            problems.push(format!("installing the hook executable: {e}"));
            return Ok(Migration {
                from: old.root.clone(),
                to: new.root.clone(),
                rewritten: Vec::new(),
                problems,
            });
        }
    };
    let _ = std::fs::remove_file(binary::legacy_installed_path(&new.root));

    let mut rewritten = Vec::new();
    for adapter in runmarq_adapters::all() {
        for scope in &scopes {
            let opts = InstallOptions {
                scope: scope.clone(),
                executable: installed.clone(),
                backups_dir: new.backups_dir(),
                home: home.clone(),
            };
            if !adapter.verify(&opts).configured {
                continue;
            }
            match adapter.install(&opts) {
                Ok(r) if r.changed => rewritten.push(r.config_path),
                Ok(_) => {}
                Err(e) => problems.push(format!("{}: {e}", adapter.source().as_str())),
            }
        }
    }

    Ok(Migration {
        from: old.root.clone(),
        to: new.root.clone(),
        rewritten,
        problems,
    })
}

fn stop_collector(old: &Paths) -> Result<(), String> {
    if !lifecycle::is_locked(old) {
        return Ok(());
    }
    if let Ok(c) = Client::discover(old, Duration::from_secs(2)) {
        let _ = c.shutdown();
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline && lifecycle::is_locked(old) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if lifecycle::is_locked(old) {
        return Err("the old collector is still running".into());
    }
    Ok(())
}

/// The user scope plus every project scope setup recorded. The store is
/// closed again before the folder is renamed.
fn recorded_scopes(old: &Paths) -> Vec<Scope> {
    let mut scopes = vec![Scope::User];
    if old.database().exists() {
        if let Ok(rows) = Store::open(&old.database()).and_then(|s| s.installations()) {
            for row in rows {
                if let Some(root) = row.scope.strip_prefix("project:") {
                    let scope = Scope::Project {
                        root: PathBuf::from(root),
                    };
                    if !scopes.contains(&scope) {
                        scopes.push(scope);
                    }
                }
            }
        }
    }
    scopes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn moves_a_pre_rename_install_and_rewrites_its_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let old = Paths::at(tmp.path().join("Runmark"));
        let new = Paths::at(tmp.path().join("runmarq"));
        let old_exe = binary::legacy_installed_path(&old.root);
        std::fs::create_dir_all(old_exe.parent().unwrap()).unwrap();
        std::fs::write(&old_exe, b"old").unwrap();
        std::fs::write(old.root.join(LEGACY_DATABASE_FILE), b"").unwrap();
        let cli = tmp.path().join("runmarq-cli");
        std::fs::write(&cli, b"new").unwrap();

        let settings = home.join(".claude").join("settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let old_cmd = format!(
            "{} emit --source claude_code --event Stop",
            old_exe.display()
        );
        std::fs::write(
            &settings,
            serde_json::to_string_pretty(&json!({
                "hooks": { "Stop": [
                    { "hooks": [{ "type": "command", "command": old_cmd }] },
                    { "hooks": [{ "type": "command", "command": "python3 mine.py" }] }
                ]}
            }))
            .unwrap(),
        )
        .unwrap();

        let m = migrate(&old, &new, &cli, Some(home.clone())).unwrap();
        assert!(m.problems.is_empty(), "{:?}", m.problems);
        assert!(!old.root.exists());
        assert!(new.root.join("runmarq.db").exists());
        assert!(!new.root.join(LEGACY_DATABASE_FILE).exists());
        assert_eq!(
            std::fs::read(binary::installed_path(&new.root)).unwrap(),
            b"new"
        );
        assert!(!binary::legacy_installed_path(&new.root).exists());
        assert_eq!(m.rewritten, vec![settings.clone()]);

        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let text = after.to_string();
        assert!(
            !text.contains("runmark.exe") && !text.contains("/runmark "),
            "{text}"
        );
        assert!(text.contains("python3 mine.py"), "other hooks are kept");
        assert!(text.contains("runmarq"));
    }

    #[test]
    fn leaves_everything_alone_when_the_new_folder_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let old = Paths::at(tmp.path().join("Runmark"));
        let new = Paths::at(tmp.path().join("runmarq"));
        std::fs::create_dir_all(&old.root).unwrap();
        std::fs::create_dir_all(&new.root).unwrap();
        assert!(migrate(&old, &new, Path::new("x"), None).is_err());
        assert!(old.root.exists());
    }
}
