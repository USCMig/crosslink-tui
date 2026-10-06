//! Snapshot -> panel text. Pure functions, so every panel renders the same way from the same data.

use crate::poll::Snap;
use serde_json::Value;
use std::fmt::Write;

pub const STAKING_PERIOD: u64 = 10_368;
pub const STAKING_WINDOW: u64 = 3_456;
pub const FIRST_WINDOW: u64 = 2 * STAKING_PERIOD;
const RULE: &str = "--------------------------------------------------------------";
const W: usize = 30;

pub fn zats(v: Option<u64>) -> String {
    match v {
        None => "-".into(),
        Some(z) => {
            let s = format!("{}.{:08}", z / 100_000_000, z % 100_000_000);
            let s = s.trim_end_matches('0').trim_end_matches('.');
            format!("{s} cTAZ")
        }
    }
}

pub fn short(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}...", &s[..n]) }
}

pub fn age(secs: i64) -> String {
    match secs {
        s if s < 0 => "-".into(),
        s if s < 120 => format!("{s}s"),
        s if s < 7200 => format!("{}m", s / 60),
        s if s < 172_800 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn utc(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "-".into())
}

fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64)
}
fn i(v: &Value, k: &str) -> Option<i64> {
    v.get(k).and_then(Value::as_i64)
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("-")
}
fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

fn line(out: &mut String, label: &str, value: impl AsRef<str>) {
    let _ = writeln!(out, "{:<W$}{}", format!("{label}:"), value.as_ref());
}

pub fn status_word(snap: &Snap) -> String {
    if !snap.node_active() {
        let state = snap.service.get("ActiveState").map(String::as_str).unwrap_or("unknown");
        return format!("Node {state}");
    }
    if let Some(e) = &snap.rpc_error {
        return format!("Starting or RPC unreachable ({})", short(e, 40));
    }
    let blocks = u(&snap.chain, "blocks").unwrap_or(0);
    let est = u(&snap.chain, "estimatedheight").unwrap_or(blocks);
    if blocks + 2 >= est {
        "Running - synced".into()
    } else {
        let pct = if est > 0 { blocks as f64 * 100.0 / est as f64 } else { 0.0 };
        format!("Syncing: {blocks}/{est} ({pct:.1}%)")
    }
}

pub fn staking_window(height: u64) -> (bool, String) {
    if height < FIRST_WINDOW {
        return (false, format!("closed - first window opens in {} blocks (height {FIRST_WINDOW})", FIRST_WINDOW - height));
    }
    let pos = height % STAKING_PERIOD;
    if pos < STAKING_WINDOW {
        (true, format!("OPEN - closes in {} blocks", STAKING_WINDOW - pos))
    } else {
        (false, format!("closed - next window opens in {} blocks", STAKING_PERIOD - pos))
    }
}

pub fn basic(snap: &Snap) -> String {
    let mut o = String::new();
    line(&mut o, "Current Status", status_word(snap));
    line(&mut o, "Connected Peers", opt(u(&snap.info, "connections")));
    line(&mut o, "Disk Usage (GB)", snap.disk_bytes.map(|b| format!("{:.3}", b as f64 / 1e9)).unwrap_or("-".into()));
    o.push_str(RULE);
    o.push('\n');
    line(&mut o, "Chain Tip Hash", short(s(&snap.chain, "bestblockhash"), 16));
    line(&mut o, "Chain Height", opt(u(&snap.chain, "blocks")));
    line(&mut o, "Header Height", opt(u(&snap.chain, "headers")));
    line(&mut o, "Difficulty", snap.chain.get("difficulty").and_then(Value::as_f64).map(|d| format!("{d:.2}")).unwrap_or("-".into()));
    line(&mut o, "Network Solutions/s", opt(u(&snap.mining, "networksolps")));
    line(&mut o, "Chain Tip Timestamp", i(&snap.header, "time").map(utc).unwrap_or("-".into()));
    o.push_str(RULE);
    o.push('\n');
    let fin_h = u(&snap.final_tip, "height");
    line(&mut o, "BFT Finalized Height", fin_h.map(|h| h.to_string()).unwrap_or("none yet".into()));
    line(&mut o, "BFT Height / Round", format!("{} / {}", opt(u(&snap.finality, "bft_height")), opt(u(&snap.finality, "bft_round"))));
    line(&mut o, "Finality Health", finality_health(&snap.finality));
    o.push_str(RULE);
    o.push('\n');
    line(&mut o, "Transaction Pool Size", format!("{} ({} bytes)", opt(u(&snap.mempool, "size")), opt(u(&snap.mempool, "bytes"))));
    o.push_str(RULE);
    o.push('\n');
    let w = &snap.wallet;
    line(&mut o, "Wallet Scan", format!("{} / {}", opt(u(w, "sync_height")), opt(u(w, "tip_height"))));
    line(&mut o, "Transparent (mined)", zats(u(w, "user_unshielded_zats")));
    line(&mut o, "Shielded spendable", zats(u(w, "user_shielded_spendable_zats")));
    line(&mut o, "Shielded pending", zats(u(w, "user_shielded_pending_zats")));
    line(&mut o, "Staked", zats(u(w, "staked_zats")));
    line(&mut o, "Withdrawable", zats(u(w, "withdrawable_zats")));
    o.push_str(RULE);
    o.push('\n');
    if let Some(t) = snap.updated {
        line(&mut o, "Updated", t.format("%H:%M:%S").to_string());
    }
    o
}

fn finality_health(f: &Value) -> String {
    match f.get("healthy").and_then(Value::as_bool) {
        None => "-".into(),
        Some(true) => "healthy".into(),
        Some(false) => {
            let n = f.get("diagnosis").and_then(Value::as_array).map_or(0, Vec::len);
            format!("NOT healthy ({n} issue{}; see BFT Finality)", if n == 1 { "" } else { "s" })
        }
    }
}

pub fn peers(snap: &Snap) -> String {
    let mut o = String::new();
    let list = snap.peers.as_array().cloned().unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let _ = writeln!(o, "PoW peers ({})\n", list.len());
    let _ = writeln!(o, "{:<24} {:<4} {:<15} {:>9} {:>9}", "Address", "Dir", "Agent", "Last recv", "Ping ms");
    for p in &list {
        let dir = if p.get("inbound").and_then(Value::as_bool).unwrap_or(false) { "in" } else { "out" };
        let last = i(p, "lastrecv").map(|t| age(now - t)).unwrap_or("-".into());
        let ping = p.get("pingtime").and_then(Value::as_f64).map(|x| format!("{:.0}", x * 1000.0)).unwrap_or("-".into());
        let _ = writeln!(o, "{:<24} {:<4} {:<15} {:>9} {:>9}", short(s(p, "addr"), 24), dir, short(s(p, "subver"), 15), last, ping);
    }
    o.push('\n');
    o.push_str(RULE);
    o.push('\n');
    let bft = snap.bft_stats.get("peers").and_then(Value::as_array).cloned().unwrap_or_default();
    let _ = writeln!(o, "BFT peers ({})\n", bft.len());
    if bft.is_empty() {
        o.push_str("None reported (BFT is not running on this node yet).\n");
    }
    for p in bft {
        let _ = writeln!(o, "{}", p.as_str().unwrap_or("-"));
    }
    o.push('\n');
    o.push_str(RULE);
    o.push('\n');
    line(&mut o, "Sync", status_word(snap));
    line(&mut o, "Blocks / Headers", format!("{} / {}", opt(u(&snap.chain, "blocks")), opt(u(&snap.chain, "headers"))));
    line(&mut o, "Estimated Height", opt(u(&snap.chain, "estimatedheight")));
    o
}

pub fn mining_status(snap: &Snap, threads_in_config: Option<i64>, gpu_env: Option<&str>) -> String {
    let mut o = String::new();
    line(&mut o, "Chain Height", opt(u(&snap.mining, "blocks")));
    line(&mut o, "Network Solutions/s", opt(u(&snap.mining, "networksolps")));
    line(&mut o, "Difficulty", snap.chain.get("difficulty").and_then(Value::as_f64).map(|d| format!("{d:.2}")).unwrap_or("-".into()));
    line(&mut o, "Threads in config", opt(threads_in_config));
    line(&mut o, "Running solvers", snap.solvers.clone().unwrap_or("-".into()));
    line(&mut o, "CPU threads on host", crate::node::cpu_count().to_string());
    line(&mut o, "Wallet transparent (mined)", zats(u(&snap.wallet, "user_unshielded_zats")));
    if let Some(env) = gpu_env.filter(|e| e.contains("GPU")) {
        line(&mut o, "GPU solver (systemd env)", "configured");
        let _ = writeln!(o, "  {env}");
    }
    o
}

pub fn staking(snap: &Snap) -> String {
    let mut o = String::new();
    let h = u(&snap.chain, "blocks").unwrap_or(0);
    let (_, win) = staking_window(h);
    line(&mut o, "Chain Height", h.to_string());
    line(&mut o, "Staking Window", win);
    line(&mut o, "My Finalizer", snap.finalizer_address.as_deref().map(|a| short(a, 40)).unwrap_or("- (not found in journal)".into()));
    let w = &snap.wallet;
    let avail = u(w, "user_shielded_spendable_zats").unwrap_or(0) + u(w, "user_unshielded_zats").unwrap_or(0);
    line(&mut o, "Available to stake", zats(Some(avail)));
    line(&mut o, "Staked", zats(u(w, "staked_zats")));
    line(&mut o, "Withdrawable", zats(u(w, "withdrawable_zats")));
    o
}

pub struct BondRow {
    pub label: String,
    pub bond_key: String,
    pub withdrawable: bool,
}

/// The wallet's own bonds, active (grouped by finalizer) then withdrawable.
pub fn bond_rows(snap: &Snap) -> Vec<BondRow> {
    let mut rows = Vec::new();
    let row = |b: &Value, finalizer: &str, withdrawable: bool| BondRow {
        label: format!(
            "{}  {:>16}  h{:<7} {}",
            short(s(b, "pk"), 12),
            zats(u(b, "latest_val")),
            opt(u(b, "create_height")),
            if withdrawable { "withdrawable".to_string() } else { format!("-> {}", short(finalizer, 12)) },
        ),
        bond_key: s(b, "pk").to_string(),
        withdrawable,
    };
    if let Some(active) = snap.positions.get("active").and_then(Value::as_object) {
        for (finalizer, bonds) in active {
            for b in bonds.as_array().into_iter().flatten() {
                rows.push(row(b, finalizer, false));
            }
        }
    }
    for b in snap.positions.get("withdrawable").and_then(Value::as_array).into_iter().flatten() {
        rows.push(row(b, "", true));
    }
    rows
}

pub struct RosterRow {
    pub label: String,
    pub address: Option<String>,
}

pub fn roster_rows(snap: &Snap) -> Vec<RosterRow> {
    let list = snap.roster.as_array().cloned().unwrap_or_default();
    let total: u64 = list.iter().filter_map(|m| u(m, "voting_power")).sum();
    let mine = snap.finalizer_address.as_deref();
    list.iter()
        .map(|m| {
            let power = u(m, "voting_power").unwrap_or(0);
            let pct = if total > 0 { power as f64 * 100.0 / total as f64 } else { 0.0 };
            let address = m.get("finalizer_address").and_then(Value::as_str).map(String::from);
            let me = if address.as_deref().is_some_and(|a| Some(a) == mine) { " (me)" } else { "" };
            RosterRow {
                label: format!(
                    "{}  {:>16}  {:>6.2}%  {}{me}",
                    short(s(m, "pub_key"), 12),
                    zats(Some(power)),
                    pct,
                    address.as_deref().map(|a| short(a, 22)).unwrap_or("no address".into()),
                ),
                address,
            }
        })
        .collect()
}

pub fn bft(snap: &Snap) -> String {
    let mut o = String::new();
    let f = &snap.finality;
    o.push_str("Finality status (get_tfl_finality_status)\n");
    if f.is_null() {
        o.push_str("  unavailable\n");
    } else {
        line(&mut o, "  Health", if f.get("healthy").and_then(Value::as_bool) == Some(true) { "healthy" } else { "NOT healthy" });
        line(&mut o, "  BFT height / round / step", format!("{} / {} / {}", opt(u(f, "bft_height")), opt(u(f, "bft_round")), s(f, "bft_step")));
        line(&mut o, "  Locked / valid round", format!("{} / {}", opt(u(f, "bft_locked_round")), opt(u(f, "bft_valid_round"))));
        line(&mut o, "  BFT chain length", opt(u(f, "bft_chain_len")));
        line(&mut o, "  PoW tip / finalized", format!("{} / {}", opt(u(f, "pow_tip_height")), opt(u(f, "finalized_height"))));
        line(&mut o, "  Finality gap", opt(u(f, "finality_gap")));
        line(&mut o, "  Since last decision", i(f, "secs_since_last_decision").map(age).unwrap_or("-".into()));
        line(&mut o, "  Roster size", opt(u(f, "roster_n")));
        line(&mut o, "  Online power", format!("{} of {} ({:.1}%)", zats(u(f, "online_power")), zats(u(f, "total_power")), f.get("online_power_pct").and_then(Value::as_f64).unwrap_or(0.0)));
        line(&mut o, "  Quorum threshold", format!("{} (online: {})", zats(u(f, "quorum_threshold")), if f.get("quorum_online").and_then(Value::as_bool) == Some(true) { "yes" } else { "no" }));
        for d in f.get("diagnosis").and_then(Value::as_array).into_iter().flatten() {
            let _ = writeln!(o, "  ! {}", d.as_str().unwrap_or(""));
        }
    }
    o.push_str(RULE);
    o.push_str("\nQuorum (get_tfl_quorum_status)\n");
    let q = snap.quorum.as_array().cloned().unwrap_or_default();
    if q.is_empty() {
        o.push_str("  no active roster\n");
    } else {
        let _ = writeln!(o, "  {:<14} {:>16} {:>7} {:<7} {:>6} {:<5} {:<5}", "Finalizer", "Power", "Share", "Online", "Seen", "Prev", "Prec");
        for m in &q {
            let me = if m.get("is_me").and_then(Value::as_bool) == Some(true) { "*" } else { " " };
            let yn = |k: &str| if m.get(k).and_then(Value::as_bool) == Some(true) { "yes" } else { "no" };
            let _ = writeln!(
                o,
                " {me}{:<14} {:>16} {:>6.2}% {:<7} {:>6} {:<5} {:<5}",
                short(s(m, "pub_key"), 10),
                zats(u(m, "voting_power")),
                m.get("power_pct").and_then(Value::as_f64).unwrap_or(0.0),
                yn("online"),
                i(m, "secs_since_seen").map(age).unwrap_or("-".into()),
                yn("prevoted"),
                yn("precommitted"),
            );
        }
    }
    o.push_str(RULE);
    o.push_str("\nRounds at current height (get_tfl_round_diagnosis)\n");
    let rounds = snap.rounds.as_array().cloned().unwrap_or_default();
    if rounds.is_empty() {
        o.push_str("  none\n");
    }
    for r in &rounds {
        let _ = writeln!(
            o,
            "  {}.{}  proposal: {} ({})  precommit {} of {} needed  votes [{}]",
            opt(u(r, "height")),
            opt(u(r, "round")),
            if r.get("proposal_present").and_then(Value::as_bool) == Some(true) { "yes" } else { "no" },
            s(r, "proposal_validity"),
            zats(u(r, "yes_precommit_power")),
            zats(u(r, "quorum_threshold")),
            s(r, "votes"),
        );
        if let Some(b) = r.get("proposal_blocked_on_block").and_then(Value::as_str) {
            let _ = writeln!(o, "     waiting on PoW block {}", short(b, 20));
        }
    }
    o.push_str(RULE);
    o.push_str("\nInternal stats (get_tfl_bft_internal_stats)\n");
    let st = &snap.bft_stats;
    line(&mut o, "  Rounds held / commit cache", format!("{} / {}", opt(u(st, "rounds_data_len")), opt(u(st, "recent_commit_round_cache_len"))));
    line(&mut o, "  BFT blocks / index", format!("{} / {}", opt(u(st, "bft_blocks_len")), opt(u(st, "bft_block_index_len"))));
    line(&mut o, "  BFT peers", st.get("peers").and_then(Value::as_array).map_or("-".into(), |p| p.len().to_string()));
    o.push_str(RULE);
    o.push_str("\nBFT tip block (get_tfl_bft_block)\n");
    let b = &snap.bft_block;
    if b.is_null() {
        o.push_str("  none yet\n");
    } else {
        line(&mut o, "  Height", opt(u(b, "height")));
        line(&mut o, "  Hash", short(s(b, "hash"), 24));
        line(&mut o, "  Finalizes PoW block", short(s(b, "candidate_hash"), 24));
        line(&mut o, "  Headers / signatures", format!("{} / {}", opt(u(b, "header_count")), opt(u(b, "signature_count"))));
    }
    o
}

pub fn node_control(snap: &Snap, service: &str) -> String {
    let mut o = String::new();
    let p = |k: &str| snap.service.get(k).cloned().unwrap_or_else(|| "-".into());
    line(&mut o, "Service", service);
    line(&mut o, "State", format!("{} ({})", p("ActiveState"), p("SubState")));
    line(&mut o, "Main PID", p("MainPID"));
    line(&mut o, "Started", p("ExecMainStartTimestamp"));
    line(&mut o, "Restarts by systemd", p("NRestarts"));
    line(&mut o, "Restart policy", p("Restart"));
    line(&mut o, "Status", status_word(snap));
    o.push_str(RULE);
    o.push_str(
        "\nStart / Stop / Restart run `sudo systemctl ...`: the TUI steps aside so you can type\n\
         your sudo password, then comes back. Quick restart needs no sudo: it sends SIGTERM and\n\
         systemd starts the node again after RestartSec (the policy above must be `always`).\n",
    );
    o
}

pub fn version(snap: &Snap, service: &str, config: &str, rpc: &str, argv: &[String]) -> String {
    let mut o = String::new();
    line(&mut o, "TUI version", env!("CARGO_PKG_VERSION"));
    line(&mut o, "zebrad build", s(&snap.info, "build"));
    line(&mut o, "zebrad subversion", s(&snap.info, "subversion"));
    line(&mut o, "Protocol version", opt(u(&snap.info, "protocolversion")));
    line(&mut o, "Chain", s(&snap.chain, "chain"));
    line(&mut o, "Service", service);
    line(&mut o, "Binary", argv.first().cloned().unwrap_or("-".into()));
    line(&mut o, "Config file", config);
    line(&mut o, "RPC", rpc);
    line(&mut o, "Unit file", snap.service.get("FragmentPath").cloned().unwrap_or("-".into()));
    line(&mut o, "Drop-ins", snap.service.get("DropInPaths").filter(|d| !d.is_empty()).cloned().unwrap_or("-".into()));
    line(&mut o, "My finalizer", snap.finalizer_address.as_deref().unwrap_or("-"));
    o
}

/// "2026-10-06T13:36:20+00:00 host zebrad[123]: 2026-10-06T13:36:20.338Z  INFO x: msg"
/// becomes "13:36:20  INFO x: msg".
fn compact_log_line(l: &str) -> String {
    let Some((head, msg)) = l.split_once("]: ") else { return l.to_string() };
    let time = head.get(11..19).unwrap_or("");
    let msg = match msg.split_once("Z  ") {
        Some((ts, rest)) if ts.len() <= 30 && ts.starts_with("20") => rest,
        _ => msg,
    };
    format!("{time} {msg}")
}

pub const LOG_FILTERS: [&str; 5] = ["All", "Warnings & errors", "Mining", "Staking & BFT", "Wallet"];

pub fn logs(snap: &Snap, filter: usize, hide_noise: bool) -> String {
    let keep = |l: &str| -> bool {
        let lower = l.to_ascii_lowercase();
        if hide_noise
            && ["new_network: tip height", "block was already queued", "congestion event packet dropped", "don't need to re-request"]
                .iter()
                .any(|k| lower.contains(k))
        {
            return false;
        }
        match filter {
            1 => l.contains(" WARN ") || l.contains(" ERROR ") || lower.contains("panic") || lower.contains("error"),
            2 => ["miner", "solver", "mined", "block template", "equihash"].iter().any(|k| lower.contains(k)),
            3 => ["stak", "bond", "bft", "tenderlink", "finaliz", "roster"].iter().any(|k| lower.contains(k)),
            4 => ["wallet", "tx build", "note", "shield"].iter().any(|k| lower.contains(k)),
            _ => true,
        }
    };
    let lines: Vec<&String> = snap.logs.iter().filter(|l| keep(l)).collect();
    let start = lines.len().saturating_sub(400);
    let mut o = String::new();
    for l in &lines[start..] {
        o.push_str(&compact_log_line(l));
        o.push('\n');
    }
    if o.is_empty() {
        o.push_str("No matching log lines yet.\n");
    }
    o
}
