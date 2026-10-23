//! Finds the agent process a hook runs under, and checks later whether it is
//! still running, so a run ends when its agent is closed without reporting it
//! (a terminal window closed, a process killed).

use runmarq_core::event::AgentProcess;

/// Programs that stand between an agent and the hooks it runs.
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "nu",
    "env",
    "cmd",
    "powershell",
    "pwsh",
];
/// Agents whose process lives exactly as long as their session. Anything
/// else, such as an editor or a terminal, may outlive the agent or end first.
const AGENTS: &[&str] = &["claude", "node", "bun", "codex"];
/// Ancestors examined before giving up.
const MAX_DEPTH: usize = 8;

/// What the operating system says about a process id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Running {
        started: u64,
    },
    Gone,
    /// It could not be checked, for example because access was denied.
    Unknown,
}

fn base_name(name: &str) -> String {
    let n = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    match n.strip_suffix(".exe") {
        Some(stem) => stem.to_string(),
        None => n,
    }
}

/// The nearest ancestor of this process that is not a shell, when it is a
/// known agent.
pub fn agent_ancestor() -> Option<AgentProcess> {
    let (pid, name) = os::ancestry(std::process::id(), MAX_DEPTH)
        .into_iter()
        .find(|(_, name)| !SHELLS.contains(&base_name(name).as_str()))?;
    if !AGENTS.contains(&base_name(&name).as_str()) {
        return None;
    }
    match os::probe(pid) {
        Probe::Running { started } => Some(AgentProcess { pid, started }),
        _ => None,
    }
}

/// False only when the process is known to have exited.
pub fn is_running(p: &AgentProcess) -> bool {
    match os::probe(p.pid) {
        Probe::Running { started } => started == p.started,
        Probe::Gone => false,
        Probe::Unknown => true,
    }
}

#[cfg(windows)]
mod os {
    use super::Probe;
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, FILETIME, INVALID_HANDLE_VALUE,
        STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// Parents of `pid`, nearest first, with their executable names.
    pub fn ancestry(pid: u32, depth: usize) -> Vec<(u32, String)> {
        let mut table = HashMap::new();
        // SAFETY: the snapshot handle is checked before use and closed after,
        // and the entry carries its own size as the API requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return Vec::new();
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e) != 0;
            while ok {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                table.insert(
                    e.th32ProcessID,
                    (
                        e.th32ParentProcessID,
                        String::from_utf16_lossy(&e.szExeFile[..len]),
                    ),
                );
                ok = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        let mut chain = Vec::new();
        let mut next = table.get(&pid).map(|(parent, _)| *parent);
        while let Some(p) = next.filter(|_| chain.len() < depth) {
            let Some((parent, name)) = table.get(&p) else {
                break;
            };
            chain.push((p, name.clone()));
            next = Some(*parent);
        }
        chain
    }

    pub fn probe(pid: u32) -> Probe {
        // SAFETY: the process handle is checked before use and closed after.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return if GetLastError() == ERROR_INVALID_PARAMETER {
                    Probe::Gone
                } else {
                    Probe::Unknown
                };
            }
            let mut code = 0u32;
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            let probe = if GetExitCodeProcess(h, &mut code) == 0
                || GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) == 0
            {
                Probe::Unknown
            } else if code != STILL_ACTIVE as u32 {
                Probe::Gone
            } else {
                Probe::Running {
                    started: (u64::from(created.dwHighDateTime) << 32)
                        | u64::from(created.dwLowDateTime),
                }
            };
            CloseHandle(h);
            probe
        }
    }
}

#[cfg(target_os = "linux")]
mod os {
    use super::Probe;

    /// Parent, name, state and start time from `/proc/<pid>/stat`.
    fn stat(pid: u32) -> std::io::Result<Option<(u32, String, char, u64)>> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let parse = || {
            let (head, tail) = s.rsplit_once(')')?;
            let name = head.split_once('(')?.1.to_string();
            // From the third field on: state, parent, ..., start time (22nd).
            let f: Vec<&str> = tail.split_whitespace().collect();
            Some((
                f.get(1)?.parse().ok()?,
                name,
                f.first()?.chars().next()?,
                f.get(19)?.parse().ok()?,
            ))
        };
        Ok(parse())
    }

    pub fn ancestry(pid: u32, depth: usize) -> Vec<(u32, String)> {
        let mut chain = Vec::new();
        let mut next = stat(pid).ok().flatten().map(|s| s.0);
        while let Some(p) = next.filter(|p| *p > 1 && chain.len() < depth) {
            let Some((parent, name, _, _)) = stat(p).ok().flatten() else {
                break;
            };
            chain.push((p, name));
            next = Some(parent);
        }
        chain
    }

    pub fn probe(pid: u32) -> Probe {
        match stat(pid) {
            Ok(Some((_, _, 'Z' | 'X', _))) => Probe::Gone,
            Ok(Some((_, _, _, started))) => Probe::Running { started },
            Ok(None) => Probe::Unknown,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Probe::Gone,
            Err(_) => Probe::Unknown,
        }
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::Probe;

    /// A zombie: exited, not yet reaped.
    const SZOMB: u32 = 5;

    fn info(pid: u32) -> Result<libc::proc_bsdinfo, i32> {
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the buffer is a zeroed struct of exactly the size this
        // flavour fills in.
        unsafe {
            let mut i: libc::proc_bsdinfo = std::mem::zeroed();
            let n = libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut i as *mut libc::proc_bsdinfo).cast(),
                size,
            );
            if n == size {
                Ok(i)
            } else {
                Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
            }
        }
    }

    pub fn ancestry(pid: u32, depth: usize) -> Vec<(u32, String)> {
        let mut chain = Vec::new();
        let mut next = info(pid).ok().map(|i| i.pbi_ppid);
        while let Some(p) = next.filter(|p| *p > 1 && chain.len() < depth) {
            let Ok(i) = info(p) else {
                break;
            };
            let name: Vec<u8> = i
                .pbi_comm
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8)
                .collect();
            chain.push((p, String::from_utf8_lossy(&name).into_owned()));
            next = Some(i.pbi_ppid);
        }
        chain
    }

    pub fn probe(pid: u32) -> Probe {
        match info(pid) {
            Ok(i) if i.pbi_status == SZOMB => Probe::Gone,
            Ok(i) => Probe::Running {
                started: i.pbi_start_tvsec * 1_000_000 + i.pbi_start_tvusec,
            },
            Err(libc::ESRCH) => Probe::Gone,
            Err(_) => Probe::Unknown,
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod os {
    use super::Probe;

    pub fn ancestry(_pid: u32, _depth: usize) -> Vec<(u32, String)> {
        Vec::new()
    }

    pub fn probe(_pid: u32) -> Probe {
        Probe::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_without_path_case_or_extension() {
        assert_eq!(base_name(r"C:\Program Files\Git\usr\bin\BASH.EXE"), "bash");
        assert_eq!(base_name("/usr/local/bin/node"), "node");
        assert_eq!(base_name("claude.exe"), "claude");
    }

    #[test]
    fn this_process_is_running_until_its_start_time_differs() {
        let pid = std::process::id();
        let Probe::Running { started } = os::probe(pid) else {
            panic!("the test process should be visible");
        };
        assert!(is_running(&AgentProcess { pid, started }));
        assert!(!is_running(&AgentProcess {
            pid,
            started: started + 1
        }));
    }

    #[test]
    fn an_exited_process_is_gone() {
        let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" })
            .args(if cfg!(windows) {
                &["/C", "exit"][..]
            } else {
                &[][..]
            })
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        drop(child);
        assert_eq!(os::probe(pid), Probe::Gone);
    }

    #[test]
    fn ancestry_starts_with_the_parent() {
        let chain = os::ancestry(std::process::id(), 3);
        assert!(!chain.is_empty());
        assert_ne!(chain[0].0, std::process::id());
    }
}
