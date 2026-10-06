//! Everything the TUI learns about the node outside of RPC: systemd state, the journal, disk use.

use std::collections::HashMap;
use std::process::Command;

const SHOW_PROPS: &str =
    "ActiveState,SubState,MainPID,ExecMainStartTimestamp,NRestarts,ExecStart,Environment,FragmentPath,DropInPaths,Restart";

pub fn service_props(service: &str) -> HashMap<String, String> {
    let out = Command::new("systemctl")
        .args(["show", service, "-p", SHOW_PROPS])
        .output();
    let mut props = HashMap::new();
    if let Ok(out) = out {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some((k, v)) = line.split_once('=') {
                props.insert(k.to_string(), v.to_string());
            }
        }
    }
    props
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

pub fn journal(service: &str, lines: usize) -> Vec<String> {
    let out = Command::new("journalctl")
        .args(["-u", service, "--no-pager", "-o", "short-iso", "-n", &lines.to_string()])
        .output();
    match out {
        Ok(out) => String::from_utf8_lossy(&out.stdout).lines().map(strip_ansi).collect(),
        Err(e) => vec![format!("journalctl failed: {e}")],
    }
}

/// Newest journal line of the current boot containing `needle` (zebrad prints some facts, like
/// its finalizer address, only at startup).
pub fn last_line_with(service: &str, needle: &str) -> Option<String> {
    let pick = |text: &str| text.lines().rev().find(|l| l.contains(needle)).map(strip_ansi);
    let grep = Command::new("journalctl")
        .args(["-u", service, "-b", "--no-pager", "-o", "cat", "-g", needle, "-n", "1"])
        .output()
        .ok()?;
    if let Some(l) = pick(&String::from_utf8_lossy(&grep.stdout)) {
        return Some(l);
    }
    // journalctl built without pattern support: scan recent lines instead.
    let all = Command::new("journalctl")
        .args(["-u", service, "-b", "--no-pager", "-o", "cat", "-n", "50000"])
        .output()
        .ok()?;
    pick(&String::from_utf8_lossy(&all.stdout))
}

pub fn finalizer_address(service: &str) -> Option<String> {
    let l = strip_ansi(&last_line_with(service, "finalizer address: ")?);
    let i = l.find("finalizer address: ")?;
    let addr: String = l[i + 19..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    addr.starts_with("zfin").then_some(addr)
}

pub fn running_solvers(service: &str) -> Option<String> {
    let l = strip_ansi(&last_line_with(service, "solver_count=")?);
    let i = l.find("solver_count=")?;
    Some(l[i + 13..].split_whitespace().next()?.to_string())
}

pub fn disk_usage_bytes(path: &str) -> Option<u64> {
    let out = Command::new("du").args(["-sb", path]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().next()?.parse().ok()
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
    let status = Command::new("kill").args(["-TERM", pid]).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("kill -TERM {pid} failed")) }
}
