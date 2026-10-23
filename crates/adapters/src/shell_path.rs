//! Makes the stable `runmarq` executable available as a command in new
//! terminals, so `runmarq wait` and `runmarq statusline` work without its
//! full path.
//!
//! On Windows the `bin` folder is added to the user's own `Path`
//! (`HKEY_CURRENT_USER\Environment`), and running programs are told that the
//! environment changed. On macOS and Linux a `runmarq` link is placed in a
//! per-user folder on the path, or `~/.local/bin` when none is.

use std::path::{Path, PathBuf};

/// Where the command was made available, and whether new terminals will
/// find it without further steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub location: PathBuf,
    pub on_path: bool,
}

/// Entries of a `Path`-style list, compared without case or trailing
/// separators on Windows.
fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().trim_end_matches(['\\', '/']);
        if cfg!(windows) {
            s.to_ascii_lowercase()
        } else {
            s.to_string()
        }
    };
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// Whether `list` (entries separated by `sep`) contains `dir`.
pub fn list_contains(list: &str, dir: &str, sep: char) -> bool {
    list.split(sep).any(|e| same_dir(e, dir))
}

/// `list` with `dir` appended, unless it is already there.
pub fn list_with(list: &str, dir: &str, sep: char) -> String {
    if list_contains(list, dir, sep) {
        return list.to_string();
    }
    let base = list.trim_end_matches(sep);
    if base.is_empty() {
        dir.to_string()
    } else {
        format!("{base}{sep}{dir}")
    }
}

/// `list` without `dir`.
pub fn list_without(list: &str, dir: &str, sep: char) -> String {
    list.split(sep)
        .filter(|e| !same_dir(e, dir))
        .collect::<Vec<_>>()
        .join(&sep.to_string())
}

#[cfg(windows)]
mod os {
    use super::{list_contains, list_with, list_without, Installed};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key was opened by `open` and is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(access: u32) -> Result<Key, String> {
        let mut key: HKEY = std::ptr::null_mut();
        let name = wide("Environment");
        // SAFETY: valid null-terminated name and out pointer.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, access, &mut key) };
        if rc != ERROR_SUCCESS {
            return Err(format!("could not open the user environment (error {rc})"));
        }
        Ok(Key(key))
    }

    /// The user's own `Path` and its registry type; empty when unset.
    fn read_path(key: &Key) -> Result<(String, u32), String> {
        let name = wide("Path");
        let mut kind = 0u32;
        let mut len = 0u32;
        // SAFETY: a null buffer asks for the size only.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok((String::new(), REG_EXPAND_SZ));
        }
        if rc != ERROR_SUCCESS {
            return Err(format!("could not read the user Path (error {rc})"));
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2) + 1];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: the buffer holds `bytes` bytes.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!("could not read the user Path (error {rc})"));
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return Err("the user Path is not text; leaving it unchanged".into());
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Ok((String::from_utf16_lossy(&buf[..end]), kind))
    }

    fn write_path(key: &Key, value: &str, kind: u32) -> Result<(), String> {
        let name = wide("Path");
        let data = wide(value);
        // SAFETY: `data` is a null-terminated UTF-16 buffer of the given size.
        let rc = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                kind,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!("could not update the user Path (error {rc})"));
        }
        let what = wide("Environment");
        let mut result = 0usize;
        // SAFETY: broadcasts a standard notification with a valid string.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                what.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                3000,
                &mut result,
            )
        };
        Ok(())
    }

    pub fn status(exe: &Path) -> bool {
        let Some(dir) = exe.parent().and_then(Path::to_str) else {
            return false;
        };
        open(KEY_QUERY_VALUE)
            .and_then(|k| read_path(&k))
            .is_ok_and(|(p, _)| list_contains(&p, dir, ';'))
    }

    pub fn install(exe: &Path) -> Result<Installed, String> {
        let dir = exe
            .parent()
            .and_then(Path::to_str)
            .ok_or("no folder for runmarq")?;
        let key = open(KEY_QUERY_VALUE | KEY_SET_VALUE)?;
        let (path, kind) = read_path(&key)?;
        if !list_contains(&path, dir, ';') {
            write_path(&key, &list_with(&path, dir, ';'), kind)?;
        }
        Ok(Installed {
            location: dir.into(),
            on_path: true,
        })
    }

    pub fn remove(exe: &Path) -> Result<(), String> {
        let dir = exe
            .parent()
            .and_then(Path::to_str)
            .ok_or("no folder for runmarq")?;
        let key = open(KEY_QUERY_VALUE | KEY_SET_VALUE)?;
        let (path, kind) = read_path(&key)?;
        if list_contains(&path, dir, ';') {
            write_path(&key, &list_without(&path, dir, ';'), kind)?;
        }
        Ok(())
    }
}

#[cfg(unix)]
mod os {
    use super::{list_contains, Installed};
    use std::path::{Path, PathBuf};

    fn path_var() -> String {
        std::env::var("PATH").unwrap_or_default()
    }

    /// Per-user folders a link may go in, preferring one already on the path.
    fn link_dir() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let local = home.join(".local").join("bin");
        let path = path_var();
        let on_path = |d: &Path| d.to_str().is_some_and(|s| list_contains(&path, s, ':'));
        [local.clone(), PathBuf::from("/usr/local/bin")]
            .into_iter()
            .find(|d| on_path(d) && can_write(d))
            .or(Some(local))
    }

    /// Whether a file can be created in `dir`.
    fn can_write(dir: &Path) -> bool {
        let probe = dir.join(format!(".runmarq-probe-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }

    fn link_in(dir: &Path) -> PathBuf {
        dir.join("runmarq")
    }

    fn points_to(link: &Path, exe: &Path) -> bool {
        std::fs::read_link(link).is_ok_and(|t| t == exe)
    }

    pub fn status(exe: &Path) -> bool {
        let path = path_var();
        path.split(':')
            .filter(|d| !d.is_empty())
            .any(|d| points_to(&link_in(Path::new(d)), exe))
    }

    pub fn install(exe: &Path) -> Result<Installed, String> {
        let dir = link_dir().ok_or("no home folder")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let link = link_in(&dir);
        match std::fs::symlink_metadata(&link) {
            Ok(m) if m.file_type().is_symlink() => {
                std::fs::remove_file(&link).map_err(|e| e.to_string())?;
            }
            Ok(_) => {
                return Err(format!(
                    "{} already exists and is not a link; leaving it unchanged",
                    link.display()
                ))
            }
            Err(_) => {}
        }
        std::os::unix::fs::symlink(exe, &link)
            .map_err(|e| format!("could not create {}: {e}", link.display()))?;
        let on_path = dir
            .to_str()
            .is_some_and(|s| list_contains(&path_var(), s, ':'));
        Ok(Installed {
            location: link,
            on_path,
        })
    }

    pub fn remove(exe: &Path) -> Result<(), String> {
        let path = path_var();
        let mut dirs: Vec<PathBuf> = path
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .collect();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local").join("bin"));
        }
        for d in dirs {
            let link = link_in(&d);
            if points_to(&link, exe) {
                std::fs::remove_file(&link).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// Whether new terminals find `exe` as `runmarq`.
pub fn status(exe: &Path) -> bool {
    os::status(exe)
}

/// Makes `exe` available as `runmarq` in new terminals.
pub fn install(exe: &Path) -> Result<Installed, String> {
    os::install(exe)
}

/// Undoes [`install`]; leaves anything it did not create alone.
pub fn remove(exe: &Path) -> Result<(), String> {
    os::remove(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lists_gain_and_lose_one_entry() {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let dir = if cfg!(windows) {
            r"C:\Users\a\AppData\Local\runmarq\bin"
        } else {
            "/home/a/.local/bin"
        };
        let base = if cfg!(windows) {
            r"C:\Windows;C:\Tools\"
        } else {
            "/usr/bin:/bin"
        };
        let with = list_with(base, dir, sep);
        assert!(list_contains(&with, dir, sep));
        assert_eq!(list_with(&with, dir, sep), with, "never added twice");
        assert_eq!(list_without(&with, dir, sep), base);
        assert_eq!(list_with("", dir, sep), dir);
        assert!(!list_contains(base, dir, sep));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_compare_without_case_or_trailing_slash() {
        assert!(list_contains(
            r"c:\users\a\appdata\local\runmarq\bin\;C:\x",
            r"C:\Users\a\AppData\Local\runmarq\bin",
            ';'
        ));
    }
}
