//! Everything the TUI learns about the node outside of RPC: the service manager, logs, disk use.
//!
//! Service control and the journal come from systemd, so they exist on Linux only. On every
//! platform the logs can instead be read from a file (`--log-file`).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command;

/// Where node log lines come from.
#[derive(Clone)]
pub enum LogSource {
    Journal(String),
    File(PathBuf),
    None,
}

impl LogSource {
    pub fn describe(&self) -> String {
        match self {
            LogSource::Journal(unit) => format!("journal of {unit}"),
            LogSource::File(p) => p.display().to_string(),
            LogSource::None => "none (pass --log-file)".into(),
        }
    }
}

/// `systemctl show` properties of the unit; empty where systemd is not available.
pub fn service_props(service: &str) -> HashMap<String, String> {
    #[cfg(target_os = "linux")]
    {
        const PROPS: &str = "LoadState,ActiveState,SubState,MainPID,ExecMainStartTimestamp,NRestarts,ExecStart,\
                             Environment,FragmentPath,DropInPaths,Restart";
        let mut props = HashMap::new();
        if let Ok(out) = Command::new("systemctl").args(["show", service, "-p", PROPS]).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some((k, v)) = line.split_once('=') {
                    props.insert(k.to_string(), v.to_string());
                }
            }
        }
        props
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = service;
        HashMap::new()
    }
}

/// True when a service manager knows this unit, so start/stop/status apply.
pub fn is_managed(props: &HashMap<String, String>) -> bool {
    props.get("LoadState").is_some_and(|s| s == "loaded")
}

/// argv of the effective ExecStart (drop-ins included), from `systemctl show`.
pub fn exec_argv(props: &HashMap<String, String>) -> Vec<String> {
    let exec = props.get("ExecStart").map(String::as_str).unwrap_or("");
    let Some(start) = exec.find("argv[]=") else { return Vec::new() };
    let rest = &exec[start + 7..];
    let end = rest.find(" ;").unwrap_or(rest.len());
    rest[..end].split_whitespace().map(String::from).collect()
}

pub fn config_path_from_argv(argv: &[String]) -> Option<String> {
    argv.iter()
        .position(|a| a == "-c" || a == "--config")
        .and_then(|i| argv.get(i + 1).cloned())
}

/// Where zebrad looks for its config when started without `-c` (its `preference_dir`).
pub fn default_config_path() -> PathBuf {
    let home = || PathBuf::from(std::env::var("HOME").unwrap_or_default());
    if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("zebrad.toml")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Preferences/zebrad.toml")
    } else {
        home().join(".config/zebrad.toml")
    }
}

pub fn default_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| if cfg!(target_os = "windows") { "notepad".into() } else { "nano".into() })
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The last `max_bytes` of a file, split into lines (the first, possibly partial, line dropped).
fn file_tail(path: &Path, max_bytes: u64) -> Vec<String> {
    let Ok(mut f) = std::fs::File::open(path) else { return vec![format!("cannot open {}", path.display())] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let mut lines: Vec<String> = String::from_utf8_lossy(&buf).lines().map(strip_ansi).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines
}

pub fn log_lines(src: &LogSource, lines: usize) -> Vec<String> {
    match src {
        LogSource::File(p) => {
            let all = file_tail(p, 4 << 20);
            all[all.len().saturating_sub(lines)..].to_vec()
        }
        #[cfg(target_os = "linux")]
        LogSource::Journal(unit) => {
            match Command::new("journalctl")
                .args(["-u", unit, "--no-pager", "-o", "short-iso", "-n", &lines.to_string()])
                .output()
            {
                Ok(out) => String::from_utf8_lossy(&out.stdout).lines().map(strip_ansi).collect(),
                Err(e) => vec![format!("journalctl failed: {e}")],
            }
        }
        _ => vec!["No log source. Start the TUI with --log-file <path to the zebrad log>.".into()],
    }
}

/// Newest log line containing `needle`. zebrad prints some facts, like its finalizer address,
/// only at startup.
pub fn last_line_with(src: &LogSource, needle: &str) -> Option<String> {
    let pick = |lines: Vec<String>| lines.into_iter().rev().find(|l| l.contains(needle));
    match src {
        LogSource::File(p) => pick(file_tail(p, 64 << 20)),
        #[cfg(target_os = "linux")]
        LogSource::Journal(unit) => {
            let run = |args: &[&str]| -> Vec<String> {
                Command::new("journalctl")
                    .args(["-u", unit, "-b", "--no-pager", "-o", "cat"])
                    .args(args)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(strip_ansi).collect())
                    .unwrap_or_default()
            };
            // journalctl may be built without pattern support; then scan recent lines instead.
            pick(run(&["-g", needle, "-n", "1"])).or_else(|| pick(run(&["-n", "50000"])))
        }
        _ => None,
    }
}

pub fn finalizer_address(src: &LogSource) -> Option<String> {
    let l = last_line_with(src, "finalizer address: ")?;
    let i = l.find("finalizer address: ")?;
    let addr: String = l[i + 19..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    addr.starts_with("zfin").then_some(addr)
}

pub fn running_solvers(src: &LogSource) -> Option<String> {
    let l = last_line_with(src, "solver_count=")?;
    let i = l.find("solver_count=")?;
    Some(l[i + 13..].split_whitespace().next()?.to_string())
}

pub fn disk_usage_bytes(path: &str) -> Option<u64> {
    fn walk(p: &Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(p) else { return 0 };
        if !meta.is_dir() {
            return meta.len();
        }
        std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| walk(&e.path())).sum()).unwrap_or(0)
    }
    let p = Path::new(path);
    p.exists().then(|| walk(p))
}

pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// SIGTERM the main process. With `Restart=always` systemd brings it back after `RestartSec`,
/// which makes this a restart that needs no root.
pub fn quick_restart(pid: &str) -> Result<(), String> {
    if pid.is_empty() || pid == "0" {
        return Err("the node is not running".into());
    }
    #[cfg(target_os = "linux")]
    {
        let status = Command::new("kill").args(["-TERM", pid]).status().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("kill -TERM {pid} failed")) }
    }
    #[cfg(not(target_os = "linux"))]
    Err("quick restart needs systemd (Linux)".into())
}
