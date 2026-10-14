//! Mining health: which blocks this miner found, read from the chain so it works against a remote
//! node too, and what the node did with its solutions, read from its logs (local only).
//!
//! zebrad logs every submit as "successfully mined a new block" with the RPC's verdict in
//! `success=`, so a rejected or duplicate block shows up under that message as well. Counts here
//! come from the verdict, not the message. "found invalid solution" lines are solutions the
//! node's own check discarded before submitting; they are counted apart from rejections.

use crate::node::{self, LogSource};
use crate::rpc::Rpc;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

/// About a day of blocks at the featurenet's 25-second target spacing.
pub const WINDOW_BLOCKS: u64 = 3_500;
/// Blocks near the tip are re-read on every pass, so a reorg replaces them.
const REORG_DEPTH: u64 = 20;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// Log lines this module reads. Kept as plain substrings so the journal's `--grep` and the file
/// fallback select the same lines.
const NEEDLES: [&str; 5] = [
    "successfully mined a new block",
    "validating a newly mined block failed",
    "found invalid solution",
    "external GPU solver",
    "GPU solver failed",
];

#[derive(Clone, Default)]
pub struct ChainStats {
    pub tip: u64,
    pub blocks_1h: u64,
    pub found_1h: u64,
    pub blocks_24h: u64,
    pub found_24h: u64,
    /// Height and block time of the newest block paying our address within the window.
    pub last_found: Option<(u64, i64)>,
    /// Oldest block height the window covers so far (the first pass fills it in).
    pub scanned_from: u64,
}

#[derive(Clone, Default)]
pub struct LogStats {
    pub accepted_1h: u64,
    pub accepted_24h: u64,
    /// Submit verdicts other than `Accepted`, with counts, over 24 h.
    pub rejected_24h: Vec<(String, u64)>,
    /// Submits that failed outright ("validating a newly mined block failed").
    pub submit_errors_24h: u64,
    pub discarded_1h: u64,
    pub discarded_24h: u64,
    /// Why discarded solutions failed the node's check, with counts, over 24 h.
    pub discarded_reasons: Vec<(String, u64)>,
    /// From the newest GPU startup line: true if the external solver started.
    pub gpu_started: Option<bool>,
    pub gpu_fallbacks_1h: u64,
    pub gpu_fallbacks_24h: u64,
    /// Accepted blocks (height, hash) whose hash is no longer the chain's block at that height.
    pub orphans: Vec<(u64, String)>,
}

#[derive(Clone, Default)]
pub struct MiningHealth {
    pub address: Option<String>,
    pub computed_at: Option<DateTime<Utc>>,
    pub chain: Option<ChainStats>,
    pub chain_error: Option<String>,
    pub logs: Option<LogStats>,
    /// Why log-based counters are missing.
    pub logs_note: Option<String>,
}

/// One cached block: hash, block time, and whether its coinbase pays our address.
type Cached = (String, i64, bool);

/// Keeps the block cache between passes, so only new and near-tip blocks are fetched.
pub struct Tracker {
    rpc: Rpc,
    log_source: LogSource,
    address: Option<String>,
    blocks: BTreeMap<u64, Cached>,
}

impl Tracker {
    pub fn new(rpc: Rpc, log_source: LogSource, address: Option<String>) -> Self {
        Self { rpc, log_source, address, blocks: BTreeMap::new() }
    }

    pub fn pass(&mut self) -> MiningHealth {
        let now = Utc::now();
        let mut h = MiningHealth { address: self.address.clone(), computed_at: Some(now), ..Default::default() };
        match self.address.clone() {
            Some(addr) => match self.scan_chain(&addr, now.timestamp()) {
                Ok(c) => h.chain = Some(c),
                Err(e) => h.chain_error = Some(e),
            },
            None => h.chain_error = Some("no miner address: set [mining] miner_address, or pass --miner-address".into()),
        }
        match log_lines_since(&self.log_source, DAY) {
            Ok(lines) => h.logs = Some(self.log_stats(&lines, now.timestamp())),
            Err(note) => h.logs_note = Some(note),
        }
        h
    }

    fn scan_chain(&mut self, addr: &str, now: i64) -> Result<ChainStats, String> {
        let tip = self.rpc.call("getblockcount", json!([]))?.as_u64().ok_or("getblockcount returned no number")?;
        let from = tip.saturating_sub(WINDOW_BLOCKS - 1).max(1);
        self.blocks.retain(|&height, _| height >= from && height <= tip);
        let refetch_from = tip.saturating_sub(REORG_DEPTH);
        for height in (from..=tip).rev() {
            if height < refetch_from && self.blocks.contains_key(&height) {
                continue;
            }
            match self.fetch(height, addr) {
                Ok(b) => {
                    self.blocks.insert(height, b);
                }
                // Keep what was read; the next pass continues from here.
                Err(e) if self.blocks.is_empty() => return Err(e),
                Err(_) => break,
            }
        }
        let mut c = ChainStats { tip, scanned_from: self.blocks.keys().next().copied().unwrap_or(tip), ..Default::default() };
        for (&height, &(_, time, ours)) in &self.blocks {
            if now - time <= DAY {
                c.blocks_24h += 1;
                c.found_24h += ours as u64;
            }
            if now - time <= HOUR {
                c.blocks_1h += 1;
                c.found_1h += ours as u64;
            }
            if ours {
                c.last_found = Some((height, time));
            }
        }
        Ok(c)
    }

    fn fetch(&self, height: u64, addr: &str) -> Result<Cached, String> {
        let b = self.rpc.call("getblock", json!([height.to_string(), 2]))?;
        let hash = b.get("hash").and_then(Value::as_str).unwrap_or_default().to_string();
        let time = b.get("time").and_then(Value::as_i64).unwrap_or(0);
        let coinbase = b.get("tx").and_then(Value::as_array).and_then(|t| t.first()).cloned().unwrap_or(Value::Null);
        // The miner's payout is the first coinbase output; later ones are funding streams.
        let payee = coinbase
            .get("vout")
            .and_then(Value::as_array)
            .and_then(|v| v.first())
            .and_then(|o| o.get("scriptPubKey"))
            .and_then(|s| s.get("addresses"))
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str);
        Ok((hash, time, payee == Some(addr)))
    }

    fn log_stats(&self, lines: &[String], now: i64) -> LogStats {
        let mut s = LogStats::default();
        let mut rejected: HashMap<String, u64> = HashMap::new();
        let mut reasons: HashMap<String, u64> = HashMap::new();
        let mut accepted: Vec<(u64, String)> = Vec::new();
        for line in lines {
            let Some(t) = line_time(line) else { continue };
            let age = now - t;
            if age > DAY {
                continue;
            }
            let recent = age <= HOUR;
            if line.contains("successfully mined a new block") {
                match field(line, "success=").as_deref() {
                    Some("Accepted") => {
                        s.accepted_24h += 1;
                        s.accepted_1h += recent as u64;
                        if let (Some(h), Some(hash)) = (height_field(line), hash_field(line)) {
                            accepted.push((h, hash));
                        }
                    }
                    Some(verdict) => *rejected.entry(verdict.to_string()).or_default() += 1,
                    None => *rejected.entry("unknown".into()).or_default() += 1,
                }
            } else if line.contains("validating a newly mined block failed") {
                s.submit_errors_24h += 1;
            } else if line.contains("found invalid solution") {
                s.discarded_24h += 1;
                s.discarded_1h += recent as u64;
                *reasons.entry(invalid_reason(line)).or_default() += 1;
            } else if line.contains("GPU solver failed") {
                s.gpu_fallbacks_24h += 1;
                s.gpu_fallbacks_1h += recent as u64;
            }
            if line.contains("internal miner is using the external GPU solver") {
                s.gpu_started = Some(true);
            } else if line.contains("could not start the GPU solver") {
                s.gpu_started = Some(false);
            }
        }
        s.rejected_24h = sorted(rejected);
        s.discarded_reasons = sorted(reasons);
        s.orphans = accepted
            .into_iter()
            .filter(|(h, hash)| self.blocks.get(h).is_some_and(|(chain_hash, _, _)| !chain_hash.eq_ignore_ascii_case(hash)))
            .collect();
        s
    }
}

fn sorted(m: HashMap<String, u64>) -> Vec<(String, u64)> {
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Log lines from the last `secs` seconds that mention mining outcomes.
fn log_lines_since(src: &LogSource, secs: i64) -> Result<Vec<String>, String> {
    let keep = |l: &String| NEEDLES.iter().any(|n| l.contains(n));
    match src {
        LogSource::File(p) => Ok(node::file_tail(p, 64 << 20).into_iter().filter(keep).collect()),
        #[cfg(target_os = "linux")]
        LogSource::Journal(unit) => {
            let since = format!("-{secs}s");
            let pattern = NEEDLES.join("|");
            let run = |extra: &[&str]| {
                std::process::Command::new("journalctl")
                    .args(["-u", unit, "--no-pager", "-o", "cat", "--since", &since])
                    .args(extra)
                    .output()
            };
            // journalctl may be built without pattern support; then filter here.
            let out = match run(&["-g", &pattern]) {
                Ok(o) if o.status.success() || o.status.code() == Some(1) => o,
                _ => run(&[]).map_err(|e| format!("journalctl failed: {e}"))?,
            };
            Ok(String::from_utf8_lossy(&out.stdout).lines().map(node::strip_ansi).filter(keep).collect())
        }
        _ => Err("submit results and discarded solutions come from the node's logs, which this TUI cannot read here (run it on the node's machine, or pass --log-file)".into()),
    }
}

/// zebrad's own RFC 3339 timestamp at the start of its log line.
fn line_time(line: &str) -> Option<i64> {
    line.split_whitespace()
        .find_map(|tok| DateTime::parse_from_rfc3339(tok).ok())
        .map(|t| t.timestamp())
}

/// The value after `key` up to the next space or comma, e.g. `success=Accepted`.
fn field(line: &str, key: &str) -> Option<String> {
    let rest = &line[line.find(key)? + key.len()..];
    let v: String = rest.chars().take_while(|c| !c.is_whitespace() && *c != ',').collect();
    (!v.is_empty()).then_some(v)
}

/// `height=Height(34865)`
fn height_field(line: &str) -> Option<u64> {
    let v = field(line, "height=Height(")?;
    v.trim_end_matches(')').parse().ok()
}

/// `hash=block::Hash("0000...")`
fn hash_field(line: &str) -> Option<String> {
    let rest = &line[line.find("hash=block::Hash(\"")? + 18..];
    Some(rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect())
}

/// The innermost error name, e.g. `DuplicateIdxs` from `error=Error(Error(DuplicateIdxs))`.
fn invalid_reason(line: &str) -> String {
    field(line, "error=")
        .map(|e| e.trim_end_matches(')').rsplit('(').next().unwrap_or("").to_string())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCEPTED: &str = r#"2026-10-09T20:00:00.000000Z  INFO zebrad::components::miner: successfully mined a new block height=Height(34865) hash=block::Hash("000049317f83d701149a945a744f38a4d9d3a5b736e781a7c3d81912653ef048") solver_id=14 success=Accepted"#;
    const DUPLICATE: &str = r#"2026-10-09T20:00:05.000000Z  INFO zebrad::components::miner: successfully mined a new block height=Height(34866) hash=block::Hash("00001111") solver_id=2 success=ErrorResponse(Duplicate)"#;
    const INVALID: &str = "2026-10-09T20:12:20.022066Z  INFO zebra_chain::work::equihash: found invalid solution for header error=Error(Error(DuplicateIdxs))";

    #[test]
    fn parses_zebrad_fields() {
        assert_eq!(line_time(ACCEPTED), Some(1791576000));
        assert_eq!(field(ACCEPTED, "success=").as_deref(), Some("Accepted"));
        assert_eq!(height_field(ACCEPTED), Some(34865));
        assert_eq!(hash_field(ACCEPTED).as_deref(), Some("000049317f83d701149a945a744f38a4d9d3a5b736e781a7c3d81912653ef048"));
        assert_eq!(invalid_reason(INVALID), "DuplicateIdxs");
    }

    #[test]
    fn counts_verdicts_not_messages_and_finds_orphans() {
        let mut t = Tracker::new(Rpc::new("http://127.0.0.1:9"), LogSource::None, None);
        // The chain now holds a different block at 34865: ours was orphaned.
        t.blocks.insert(34865, ("0000aaaa".into(), 1791576000, false));
        let lines: Vec<String> = [ACCEPTED, DUPLICATE, INVALID].iter().map(|s| s.to_string()).collect();
        let s = t.log_stats(&lines, 1791576000 + 1800);
        assert_eq!(s.accepted_24h, 1);
        assert_eq!(s.accepted_1h, 1);
        assert_eq!(s.rejected_24h, vec![("ErrorResponse(Duplicate)".to_string(), 1)]);
        assert_eq!(s.discarded_24h, 1);
        assert_eq!(s.discarded_1h, 1);
        assert_eq!(s.discarded_reasons, vec![("DuplicateIdxs".to_string(), 1)]);
        assert_eq!(s.orphans, vec![(34865, "000049317f83d701149a945a744f38a4d9d3a5b736e781a7c3d81912653ef048".to_string())]);
    }

    #[test]
    fn old_lines_are_ignored() {
        let t = Tracker::new(Rpc::new("http://127.0.0.1:9"), LogSource::None, None);
        let s = t.log_stats(&[ACCEPTED.to_string()], 1791576000 + 2 * DAY);
        assert_eq!(s.accepted_24h, 0);
    }
}
