//! Snapshot -> panel text. Pure functions, so every panel renders the same way from the same data.

use crate::poll::Snap;
use cursive::theme::{BaseColor, Color, ColorStyle};
use cursive::utils::markup::StyledString;
use serde_json::Value;
use std::fmt::Write;

pub const STAKING_PERIOD: u64 = 10_368;
pub const STAKING_WINDOW: u64 = 3_456;
pub const FIRST_WINDOW: u64 = 2 * STAKING_PERIOD;
const RULE: &str = "--------------------------------------------------------------";
const W: usize = 30;

fn ctaz_num(z: u64) -> String {
    let s = format!("{}.{:08}", z / 100_000_000, z % 100_000_000);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn zats(v: Option<u64>) -> String {
    match v {
        None => "-".into(),
        Some(z) => format!("{} cTAZ", ctaz_num(z)),
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

/// RPCs this TUI uses that the upstream node releases (v14, v15) do not have. The README explains
/// how to add them to a node build (node-patches/).
pub const EXTRA_RPCS: [&str; 6] = [
    "get_tfl_finality_status",
    "get_tfl_quorum_status",
    "get_tfl_round_diagnosis",
    "get_tfl_bft_internal_stats",
    "get_tfl_bft_block",
    "get_wallet_sync_status",
];

fn lacks(snap: &Snap, rpc: &str) -> bool {
    snap.missing_rpcs.contains(rpc)
}

fn lacks_note(rpc: &str) -> String {
    format!("n/a: this node has no {rpc} RPC (not in the upstream releases; see the README)")
}

fn line(out: &mut String, label: &str, value: impl AsRef<str>) {
    let _ = writeln!(out, "{:<W$}{}", format!("{label}:"), value.as_ref());
}

pub fn status_word(snap: &Snap) -> String {
    if snap.managed && !snap.node_active() {
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
    line(&mut o, "Node Release", crate::upgrade::node_release(snap.node_commit.as_deref(), &snap.info, &snap.chain));
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
    let tip = u(&snap.chain, "blocks").unwrap_or(0);
    let (up, warn) = crate::upgrade::status_line(&snap.info, &snap.chain, tip, snap.avg_block_secs);
    let _ = writeln!(o, "{}{:<W2$}{}", if warn { "!" } else { "" }, "Network Upgrade:", up.trim_start_matches("! "), W2 = if warn { W - 1 } else { W });
    o.push_str(RULE);
    o.push('\n');
    let fin_h = u(&snap.final_tip, "height");
    line(&mut o, "BFT Finalized Height", fin_h.map(|h| h.to_string()).unwrap_or("none yet".into()));
    if lacks(snap, "get_tfl_finality_status") {
        // Every node has the recency status, so the position is still known without the diagnostics.
        line(&mut o, "BFT Height / Round", format!("{} / {}", opt(u(&snap.recency, "my_height")), opt(u(&snap.recency, "my_round"))));
        let health = match bft_stall_secs(snap) {
            Some(secs) if secs >= STALL_SECS => format!("no decision seen for {} (see BFT Finality)", age(secs)),
            Some(_) => "deciding".into(),
            None => "-".into(),
        };
        line(&mut o, "Finality Health", health);
    } else {
        line(&mut o, "BFT Height / Round", format!("{} / {}", opt(u(&snap.finality, "bft_height")), opt(u(&snap.finality, "bft_round"))));
        line(&mut o, "Finality Health", finality_health(&snap.finality));
    }
    o.push_str(RULE);
    o.push('\n');
    line(&mut o, "Transaction Pool Size", format!("{} ({} bytes)", opt(u(&snap.mempool, "size")), opt(u(&snap.mempool, "bytes"))));
    o.push_str(RULE);
    o.push('\n');
    let w = &snap.wallet;
    if snap.wallet_fallback {
        line(&mut o, "Wallet Scan", lacks_note("get_wallet_sync_status"));
        line(&mut o, "Balances from", "wallet_spendable_funds, wallet_staking_positions");
    } else {
        line(&mut o, "Wallet Scan", format!("{} / {}", opt(u(w, "sync_height")), opt(u(w, "tip_height"))));
    }
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
    if lacks(snap, "get_tfl_bft_internal_stats") {
        let _ = writeln!(o, "{}", lacks_note("get_tfl_bft_internal_stats"));
    } else if bft.is_empty() {
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
    mining_health(&mut o, snap, gpu_env.is_some_and(|e| e.contains("GPU")));
    o
}

fn count_list(items: &[(String, u64)]) -> String {
    items.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
}

fn share(found: u64, of: u64) -> String {
    if of == 0 { "-".into() } else { format!("{found} of {of} ({:.2}%)", found as f64 * 100.0 / of as f64) }
}

/// Blocks found (from the chain) and what happened to submitted and discarded solutions (from
/// the logs). Lines that need attention start with `!`.
fn mining_health(o: &mut String, snap: &Snap, gpu_configured: bool) {
    let _ = writeln!(o, "{RULE}");
    let Some(h) = &snap.mining_health else {
        let _ = writeln!(o, "Mining health: first scan running (reads about a day of blocks)...");
        return;
    };
    let updated = h.computed_at.map(|t| age((chrono::Utc::now() - t).num_seconds())).unwrap_or("-".into());
    let _ = writeln!(o, "Mining health (updated {updated} ago; refreshes every minute)");
    line(o, "Payout address", h.address.as_deref().map(|a| short(a, 40)).unwrap_or("- (none set)".into()));

    match (&h.chain, &h.chain_error) {
        (Some(c), _) => {
            let net = u(&snap.mining, "networksolps").unwrap_or(0) as f64;
            line(o, "Blocks found, last hour", share(c.found_1h, c.blocks_1h));
            line(o, "Blocks found, last day", share(c.found_24h, c.blocks_24h));
            if net > 0.0 && c.blocks_24h > 0 {
                let est = |f: u64, b: u64| if b == 0 { "-".into() } else { format!("~{:.0}", net * f as f64 / b as f64) };
                line(
                    o,
                    "Est. Sol/s from block share",
                    format!("{} (hour), {} (day); rough with few blocks", est(c.found_1h, c.blocks_1h), est(c.found_24h, c.blocks_24h)),
                );
            }
            let last = match c.last_found {
                Some((height, t)) => format!("{height}, {} ago", age(chrono::Utc::now().timestamp() - t)),
                None => "none in the window".into(),
            };
            line(o, "Last block found", last);
            if c.tip >= crate::mining::WINDOW_BLOCKS && c.scanned_from > c.tip + 1 - crate::mining::WINDOW_BLOCKS {
                let _ = writeln!(o, "  (still reading the last day of blocks: back to {} so far)", c.scanned_from);
            } else if c.found_24h == 0 && c.blocks_24h > 0 {
                let _ = writeln!(o, "! No block found in the last day. Check that the miner is running and paying this address.");
            }
        }
        (None, Some(e)) => line(o, "Blocks found", format!("unavailable: {e}")),
        (None, None) => {}
    }

    let Some(l) = &h.logs else {
        if let Some(note) = &h.logs_note {
            let _ = writeln!(o, "Log-based counters unavailable: {note}.");
        }
        return;
    };
    let _ = writeln!(o, "From the node's logs:");
    line(o, "  Submitted, accepted", format!("{} (hour), {} (day)", l.accepted_1h, l.accepted_24h));
    if l.rejected_24h.is_empty() {
        line(o, "  Submitted, rejected", "none (day)");
    } else {
        let n: u64 = l.rejected_24h.iter().map(|r| r.1).sum();
        let _ = writeln!(o, "! {:<28}{n} (day): {}", "Submitted, rejected:", count_list(&l.rejected_24h));
    }
    if l.submit_errors_24h > 0 {
        let _ = writeln!(o, "! {:<28}{} (day)", "Submit errors:", l.submit_errors_24h);
    }
    if l.orphans.is_empty() {
        line(o, "  Orphaned after acceptance", "none (day)");
    } else {
        let list: Vec<String> = l.orphans.iter().map(|(height, hash)| format!("{height} ({})", short(hash, 12))).collect();
        let _ = writeln!(o, "! {:<28}{}: {}", "Orphaned after acceptance:", l.orphans.len(), list.join(", "));
    }
    let reasons = if l.discarded_reasons.is_empty() { String::new() } else { format!(": {}", count_list(&l.discarded_reasons)) };
    line(o, "  Discarded before submit", format!("{} (hour), {} (day){reasons}", l.discarded_1h, l.discarded_24h));
    let _ = writeln!(o, "    The node's own check threw these solutions away; the built-in CPU solver produces a few.");
    let gpu = match l.gpu_started {
        Some(true) if l.gpu_fallbacks_1h > 0 => {
            format!("! running, but fell back to the CPU {} times (hour), {} (day)", l.gpu_fallbacks_1h, l.gpu_fallbacks_24h)
        }
        Some(true) if l.gpu_fallbacks_24h > 0 => {
            format!("running; no fallbacks this hour ({} earlier in the day, e.g. before a restart)", l.gpu_fallbacks_24h)
        }
        Some(true) => "running, no fallbacks to the CPU (day)".to_string(),
        Some(false) => "! failed to start; the node mines on the CPU only".to_string(),
        None if gpu_configured => "configured, no startup line in the last day".to_string(),
        None => "not configured".to_string(),
    };
    if gpu.starts_with('!') {
        let _ = writeln!(o, "! {:<28}{}", "GPU solver:", gpu.trim_start_matches("! "));
    } else {
        line(o, "  GPU solver", gpu);
    }
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
    if crate::upgrade::node_is_v15(&snap.info, &snap.chain) {
        let _ = writeln!(o, "{RULE}");
        let _ = writeln!(o, "v15 rules: a new bond must be a power of 10 zats (1, 10, 100, 1000 cTAZ...).");
        let _ = writeln!(o, "Unbonding and withdrawing take any amount. Fill max picks the largest bond that fits.");
    }
    let (up, warn) = crate::upgrade::status_line(&snap.info, &snap.chain, h, snap.avg_block_secs);
    if warn {
        let _ = writeln!(o, "{up}");
    }
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

/// How many bonded finalizers the roster snapshot keeps as the voting set. Only a projection until
/// the node reports a roster: rank on the bonded-stake board is not a committee seat.
const COMMITTEE_N: usize = 12;
const BAR_W: usize = 24;
const POOL_W: usize = 40;
/// Freshness limit for the public board when the document does not state `max_age_seconds`.
const DEFAULT_BOARD_MAX_AGE_SECS: i64 = 75;

struct Seat {
    pk: String,
    addr: Option<String>,
    label: Option<String>,
    amount: u64,
    bonds: u64,
    me: bool,
}

/// One voting-roster member as this node reports it (`get_tfl_roster_zats`).
struct Member {
    /// Lowercase hex in the RPC's raw byte order.
    pk: String,
    addr: Option<String>,
    power: u64,
}

fn committee(snap: &Snap) -> Vec<Member> {
    let mut members: Vec<Member> = snap
        .roster
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Member {
            pk: s(m, "pub_key").to_ascii_lowercase(),
            addr: m.get("finalizer_address").and_then(Value::as_str).map(String::from),
            power: u(m, "voting_power").unwrap_or(0),
        })
        .collect();
    members.sort_by(|a, b| b.power.cmp(&a.power).then_with(|| b.pk.cmp(&a.pk)));
    members
}

fn reversed_hex(hex: &str) -> String {
    let bytes: Vec<&str> = (0..hex.len()).step_by(2).filter_map(|i| hex.get(i..i + 2)).collect();
    bytes.into_iter().rev().collect()
}

/// The roster rank (1-based) of a board row. The board and the roster RPC may print the public key
/// in opposite byte orders, so the finalizer address is compared first and the key both ways.
fn seat_of(seat: &Seat, members: &[Member]) -> Option<usize> {
    members.iter().position(|m| {
        matches!((&seat.addr, &m.addr), (Some(a), Some(b)) if a == b) || m.pk == seat.pk || m.pk == reversed_hex(&seat.pk)
    }).map(|i| i + 1)
}

fn secs_since(t: chrono::DateTime<chrono::Utc>) -> i64 {
    (chrono::Utc::now() - t).num_seconds().max(0)
}

/// How old the board is and whether it is past the index's own freshness limit.
struct BoardAge {
    expired: bool,
    text: String,
}

fn board_age(snap: &Snap) -> BoardAge {
    let doc = &snap.board;
    let st = doc.get("staking").unwrap_or(&Value::Null);
    let observed = st
        .get("observed_at")
        .or_else(|| doc.get("observed_at"))
        .and_then(Value::as_str)
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&chrono::Utc))
        .or(snap.board_fetched_at);
    let max_age = st
        .get("max_age_seconds")
        .or_else(|| doc.get("max_age_seconds"))
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_BOARD_MAX_AGE_SECS);
    let height = json_u64(st.get("height")).or_else(|| json_u64(doc.get("height")));
    let tip = if snap.rpc_error.is_none() { u(&snap.chain, "blocks") } else { None };

    let Some(observed) = observed else {
        return BoardAge { expired: true, text: "Data age unknown: the index did not say when it observed the chain.".into() };
    };
    let secs = secs_since(observed);
    let block = match (height, tip) {
        (Some(h), Some(t)) if t > h => format!("block {h} ({} behind your node)", t - h),
        (Some(h), Some(_)) => format!("block {h} (matches your node)"),
        (Some(h), None) => format!("block {h}"),
        (None, _) => "an unknown block".into(),
    };
    if secs > max_age {
        BoardAge {
            expired: true,
            text: format!(
                "EXPIRED: observed {} ago at {block}, past the index's {max_age}s limit. This is the last list the index served and may no longer match the chain.",
                age(secs)
            ),
        }
    } else {
        BoardAge { expired: false, text: format!("As of {block}, observed {} ago (valid for {max_age}s).", age(secs)) }
    }
}

/// Bonded-stake leaderboard from the public cTAZ roster document.
///
/// Rank here is bonded stake only. Committee membership comes from the node's own voting roster
/// (`get_tfl_roster_zats`), which stays empty until BFT starts; until then the top-`COMMITTEE_N`
/// line is a projection. Finalizer addresses are matched to mark our row and never drawn.
pub fn top_stakers(snap: &Snap) -> StyledString {
    let mut out = StyledString::new();
    let plain = ColorStyle::primary();
    let dim = fg(Color::Light(BaseColor::Black));
    let warn = ColorStyle::title_secondary();
    let green = fg(Color::Light(BaseColor::Green));
    let yellow = fg(Color::Light(BaseColor::Yellow));
    let cyan = fg(Color::Light(BaseColor::Cyan));
    let magenta = fg(Color::Light(BaseColor::Magenta));

    out.append_styled("Bonded stake ranking", ColorStyle::title_primary());
    out.append_styled(" from the public index (ctaz.cash). Rank is by bonded stake; it is not committee membership.\n", dim);

    if snap.board.is_null() {
        match &snap.board_error {
            Some(e) => out.append_styled(format!("Board unavailable: {}.\n", brief(e)), warn),
            None => out.append_plain("Fetching...\n"),
        }
        return out;
    }

    let doc = &snap.board;
    let staking = doc.get("staking").cloned().unwrap_or(Value::Null);
    let freshness = board_age(snap);
    out.append_styled(format!("{}\n", freshness.text), if freshness.expired { warn } else { dim });

    let members = committee(snap);
    let known = !members.is_empty();
    if known {
        out.append_styled(
            format!("Committee: {} seats in your node's voting roster. The `seat` column is actual membership.\n", members.len()),
            dim,
        );
    } else {
        let ms = doc.get("milestones");
        let at = |k: &str| ms.and_then(|m| json_u64(m.get(k)));
        let when = match (at("roster_snapshot"), at("bft_activation")) {
            (Some(r), Some(b)) => format!(" (roster snapshot at block {r}; BFT starts at block {b})"),
            (None, Some(b)) => format!(" (BFT starts at block {b})"),
            _ => String::new(),
        };
        out.append_styled(
            format!("Committee: not formed yet. Your node reports no voting roster{when}. The top-{COMMITTEE_N} line below is a projection by bonded stake, not confirmed seats.\n"),
            warn,
        );
    }

    let mut labels = std::collections::HashMap::<String, String>::new();
    for id in doc.get("published_identities").and_then(Value::as_array).into_iter().flatten() {
        let Some(pk) = id.get("public_key").and_then(Value::as_str) else { continue };
        let Some(label) = id.get("label").and_then(Value::as_str).filter(|s| !s.is_empty()) else { continue };
        labels.insert(pk.to_ascii_lowercase(), label.to_string());
    }

    let mine = snap.finalizer_address.as_deref();
    let mut rows = Vec::new();
    for f in staking.get("finalizers").and_then(Value::as_array).into_iter().flatten() {
        let Some(pk) = f.get("public_key").and_then(Value::as_str) else { continue };
        let addr = f.get("finalizer_address").and_then(Value::as_str);
        rows.push(Seat {
            label: labels.get(&pk.to_ascii_lowercase()).cloned(),
            pk: pk.to_ascii_lowercase(),
            addr: addr.map(String::from),
            amount: json_u64(f.get("active_amount_zats")).unwrap_or(0),
            bonds: json_u64(f.get("active_bond_count")).unwrap_or(0),
            me: match (mine, addr) {
                (Some(m), Some(a)) => a.eq_ignore_ascii_case(m),
                _ => false,
            },
        });
    }
    // Same order the snapshot uses: bonded amount, then public key, both descending.
    rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| b.pk.cmp(&a.pk)));
    let total: u64 = rows.iter().map(|r| r.amount).sum();
    let leader = rows.first().map(|r| r.amount).unwrap_or(0);

    if rows.is_empty() {
        // A retrying index sends finalizers: null. An observed empty array is the real zero.
        let listed = staking.get("finalizers").and_then(Value::as_array).is_some();
        let failed = staking.get("ok").and_then(Value::as_bool) == Some(false);
        if listed && !failed {
            out.append_plain("No bonds.\n");
        } else {
            out.append_plain("Index updating.\n");
        }
        append_board_error(&mut out, snap, warn);
        return out;
    }
    if staking.get("complete").and_then(Value::as_bool) == Some(false) {
        out.append_styled("still indexing\n", warn);
    }
    if mine.is_none() {
        out.append_styled("row not marked yet\n", dim);
    } else if !rows.iter().any(|r| r.me) {
        out.append_plain("no bond on this board\n");
    }
    out.append_plain("\n");

    // An expired board is drawn entirely in the dim style, so it cannot pass for live data.
    let live = |style: ColorStyle| if freshness.expired { dim } else { style };

    // Stacked bar for the pool, then one bar per finalizer scaled to the leader.
    let shown = rows.len().min(4);
    let rest: u64 = rows.iter().skip(shown).map(|r| r.amount).sum();
    let mut parts: Vec<u64> = rows.iter().take(shown).map(|r| r.amount).collect();
    if rest > 0 {
        parts.push(rest);
    }
    let widths = alloc_widths(&parts, POOL_W);
    let palette = [yellow, cyan, plain, magenta];
    for (i, w) in widths.iter().copied().enumerate() {
        let style = if i < shown && rows[i].me {
            green
        } else if rest > 0 && i + 1 == widths.len() {
            dim
        } else {
            palette.get(i).copied().unwrap_or(dim)
        };
        paint(&mut out, w, solid_from(live(style)));
    }
    out.append_plain("\n");
    for i in 0..shown {
        let style = if rows[i].me { green } else { palette[i] };
        let name = legend_name(&rows[i]);
        paint(&mut out, 2, solid_from(live(style)));
        out.append_styled(format!(" {name} {:.1}%  ", pct(rows[i].amount, total)), live(style));
    }
    if rest > 0 {
        paint(&mut out, 2, solid_from(dim));
        out.append_styled(format!(" other {:.1}%", pct(rest, total)), dim);
    }
    out.append_plain("\n");
    out.append_styled(format!("{:>3}  {:<22} {:>14} {:>7} {:>5}  {:<8} \n", "#", "name", "cTAZ", "share", "bonds", "seat"), dim);

    for (i, row) in rows.iter().enumerate() {
        let rank = i + 1;
        if i == COMMITTEE_N {
            let note = if known { "by bonded stake" } else { "by bonded stake: projected cutoff, not confirmed seats" };
            out.append_styled(format!("── top {COMMITTEE_N} {note} ──\n"), dim);
        }
        let seat = seat_of(row, &members);
        let in_set = if known { seat.is_some() } else { rank <= COMMITTEE_N };
        let text_style = if row.me { green } else if in_set { plain } else { dim };
        let bar_style = if row.me {
            green
        } else if rank == 1 {
            yellow
        } else if in_set {
            cyan
        } else {
            dim
        };
        let seat_text = match (known, seat) {
            (false, _) => "-".to_string(),
            (true, Some(n)) => format!("#{n}"),
            (true, None) => "none".to_string(),
        };
        let filled = blocks(row.amount, leader, BAR_W);
        out.append_styled(
            format!(
                "{rank:>3}  {} {:>14} {:>6.2}% {:>5}  {:<8} ",
                fit(&seat_name(row), 22),
                ctaz_num(row.amount),
                pct(row.amount, total),
                row.bonds,
                seat_text,
            ),
            live(text_style),
        );
        // Both parts are spaces. The fill is the bar color and the rest is a darker trough,
        // so a font that draws U+2588 and U+2591 as the same dash cannot flatten the chart.
        paint(&mut out, filled, solid_from(live(bar_style)));
        paint(&mut out, BAR_W - filled, solid(BAR_TRACK));
        out.append_plain("\n");
    }

    if known {
        let total_power: u64 = members.iter().map(|m| m.power).sum();
        out.append_plain("\n");
        out.append_styled("Committee from your node (get_tfl_roster_zats), by voting power\n", ColorStyle::title_primary());
        out.append_styled(format!("{:>4}  {:<22} {:>16} {:>7}  {}\n", "seat", "finalizer", "voting power", "share", "bonded rank"), dim);
        for (i, m) in members.iter().enumerate() {
            let board_rank = rows.iter().position(|r| seat_of(r, std::slice::from_ref(m)).is_some()).map(|r| r + 1);
            let name = board_rank
                .map(|r| seat_name(&rows[r - 1]))
                .unwrap_or_else(|| m.pk.chars().take(12).collect());
            let me = matches!((&m.addr, mine), (Some(a), Some(b)) if a == b);
            let rank_text = match board_rank {
                Some(r) if r == i + 1 => format!("#{r}"),
                Some(r) => format!("#{r} (differs)"),
                None => "not on board".into(),
            };
            out.append_styled(
                format!("{:>4}  {} {:>16} {:>6.2}%  {rank_text}\n", format!("#{}", i + 1), fit(&name, 22), zats(Some(m.power)), pct(m.power, total_power)),
                if me { green } else { plain },
            );
        }
    }
    append_board_error(&mut out, snap, warn);
    out
}

fn append_board_error(out: &mut StyledString, snap: &Snap, style: ColorStyle) {
    if let Some(e) = &snap.board_error {
        let since = snap
            .board_fetched_at
            .map(|t| format!(" from {} ago", age(secs_since(t))))
            .unwrap_or_default();
        out.append_styled(format!("Last refresh failed: {}. Showing the previous board{since}.\n", brief(e)), style);
    }
}

/// A warning for panels built from node RPC data while the node is not answering.
pub fn node_stale_note(snap: &Snap) -> Option<String> {
    snap.rpc_error.as_ref()?;
    Some(match snap.rpc_ok_at {
        Some(t) => format!(
            "NOT LIVE: the node RPC has not answered for {}. Values below are from {} and may be out of date.",
            age(secs_since(t)),
            t.with_timezone(&chrono::Local).format("%H:%M:%S")
        ),
        None => "NOT LIVE: the node RPC has not answered since the TUI started. Nothing below is current.".into(),
    })
}

/// Prefixes a node-derived panel with the not-live warning when the node RPC is down.
pub fn with_stale_note(snap: &Snap, body: String) -> StyledString {
    let mut out = StyledString::new();
    if let Some(note) = node_stale_note(snap) {
        out.append_styled(format!("{note}\n{RULE}\n"), ColorStyle::title_secondary());
    }
    out.append_plain(body);
    out
}

fn fg(color: Color) -> ColorStyle {
    ColorStyle::new(color, Color::TerminalDefault)
}

/// A cell whose foreground and background are the same color. A space drawn this way is a solid
/// swatch. U+2588 FULL BLOCK and U+2591 LIGHT SHADE are one glyph in some fonts, so a bar made
/// of them is the same length on every row.
fn solid(color: Color) -> ColorStyle {
    ColorStyle::new(color, color)
}

fn solid_from(style: ColorStyle) -> ColorStyle {
    ColorStyle { front: style.front, back: style.front }
}

fn paint(out: &mut StyledString, cells: usize, style: ColorStyle) {
    if cells > 0 {
        out.append_styled(" ".repeat(cells), style);
    }
}

/// Empty part of a row bar. Dark gray from the 256-color cube, so it still reads as a trough
/// where truecolor is unavailable, and it does not depend on a shade glyph.
const BAR_TRACK: Color = Color::RgbLowRes(1, 1, 1);

fn brief(e: &str) -> String {
    let mut s: String = e.chars().filter(|c| *c != '\n').take(160).collect();
    if e.chars().filter(|c| *c != '\n').count() > 160 {
        s.push_str("...");
    }
    s
}

fn json_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { part as f64 * 100.0 / total as f64 }
}

fn seat_name(row: &Seat) -> String {
    match (&row.label, row.me) {
        (Some(label), true) => format!("* {label}"),
        (Some(label), false) => label.clone(),
        (None, true) => "* you".into(),
        (None, false) => row.pk.chars().take(12).collect(),
    }
}

fn legend_name(row: &Seat) -> String {
    if row.me {
        "you".into()
    } else if let Some(label) = &row.label {
        fit(label, 16).trim_end().to_string()
    } else {
        row.pk.chars().take(8).collect()
    }
}

fn fit(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n > width {
        let mut t: String = s.chars().take(width.saturating_sub(1)).collect();
        t.push('…');
        t
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

fn blocks(part: u64, whole: u64, width: usize) -> usize {
    if whole == 0 || part == 0 || width == 0 {
        return 0;
    }
    ((part as u128 * width as u128) / whole as u128).clamp(1, width as u128) as usize
}

/// Largest-remainder widths that sum to `width`.
fn alloc_widths(amounts: &[u64], width: usize) -> Vec<usize> {
    let total: u128 = amounts.iter().map(|a| *a as u128).sum();
    if total == 0 || width == 0 {
        return vec![0; amounts.len()];
    }
    let mut widths: Vec<usize> = amounts.iter().map(|a| ((*a as u128 * width as u128) / total) as usize).collect();
    let mut used: usize = widths.iter().sum();
    let mut order: Vec<usize> = (0..amounts.len()).collect();
    order.sort_by(|&i, &j| {
        let ri = (amounts[i] as u128 * width as u128) % total;
        let rj = (amounts[j] as u128 * width as u128) % total;
        rj.cmp(&ri).then(i.cmp(&j))
    });
    for i in order {
        if used >= width {
            break;
        }
        widths[i] += 1;
        used += 1;
    }
    widths
}

/// How long BFT has gone without deciding, from the diagnostics when present, else from this
/// TUI's own observations (a lower bound until it has watched for a while).
pub fn bft_stall_secs(snap: &Snap) -> Option<i64> {
    if let Some(secs) = i(&snap.finality, "secs_since_last_decision") {
        return Some(secs);
    }
    let now = chrono::Utc::now().timestamp();
    snap.history.height_since().map(|t| now - t)
}

/// BFT is considered stalled once it has gone this long without a decision.
pub const STALL_SECS: i64 = 300;

/// Short BFT state for the header: height, and a warning when it stalls.
pub fn bft_header(snap: &Snap) -> Option<(String, bool)> {
    let height = u(&snap.finality, "bft_height").or_else(|| u(&snap.recency, "my_height"))?;
    if height == 0 {
        return Some(("BFT not started".into(), false));
    }
    match bft_stall_secs(snap) {
        Some(secs) if secs >= STALL_SECS => Some((format!("BFT STALLED at {height} for {}", age(secs)), true)),
        _ => Some((format!("BFT {height}"), false)),
    }
}

pub fn bft(snap: &Snap) -> String {
    let mut o = String::new();
    let f = &snap.finality;
    let r = &snap.recency;
    let now = chrono::Utc::now().timestamp();
    let diag = !lacks(snap, "get_tfl_finality_status") && !f.is_null();

    // Summary: from the diagnostics when the node has them, otherwise from RPCs every node has.
    o.push_str("Summary\n");
    let height = u(f, "bft_height").or_else(|| u(r, "my_height"));
    let round = u(f, "bft_round").or_else(|| u(r, "my_round"));
    let step = f.get("bft_step").and_then(Value::as_str).map(String::from).or_else(|| u(r, "my_step").map(|x| crate::finality::step_name(x).to_string()));
    let health = if diag {
        if f.get("healthy").and_then(Value::as_bool) == Some(true) { "healthy".to_string() } else { "NOT healthy".to_string() }
    } else {
        match bft_stall_secs(snap) {
            Some(secs) if secs >= STALL_SECS => format!("NOT healthy: no decision seen for {}", age(secs)),
            Some(_) => "deciding".to_string(),
            None => "-".to_string(),
        }
    };
    line(&mut o, "  Health", health);
    line(&mut o, "  BFT height / round / step", format!("{} / {} / {}", opt(height), opt(round), step.as_deref().unwrap_or("-")));
    let since = i(f, "secs_since_last_decision").or_else(|| snap.history.last_decision_seen().map(|t| now - t));
    line(&mut o, "  Since last decision", since.map(age).unwrap_or("- (none seen yet)".into()));
    let tip = u(&snap.chain, "blocks");
    let fin = u(&snap.final_tip, "height");
    line(&mut o, "  PoW tip / finalized", format!("{} / {}", opt(tip), fin.map(|h| h.to_string()).unwrap_or("none yet".into())));
    if let (Some(t), Some(fh)) = (tip, fin) {
        line(&mut o, "  Finality gap", format!("{} blocks", t.saturating_sub(fh)));
    }
    if diag {
        line(&mut o, "  Locked / valid round", format!("{} / {}", opt(i(f, "bft_locked_round")), opt(i(f, "bft_valid_round"))));
        line(&mut o, "  Online power", format!("{} of {} ({:.1}%)", zats(u(f, "online_power")), zats(u(f, "total_power")), f.get("online_power_pct").and_then(Value::as_f64).unwrap_or(0.0)));
        line(&mut o, "  Quorum threshold", format!("{} (online: {})", zats(u(f, "quorum_threshold")), if f.get("quorum_online").and_then(Value::as_bool) == Some(true) { "yes" } else { "no" }));
        for d in f.get("diagnosis").and_then(Value::as_array).into_iter().flatten() {
            let _ = writeln!(o, "  ! {}", d.as_str().unwrap_or(""));
        }
    } else {
        let _ = writeln!(o, "  (from get_tfl_recency_status; the diagnostic RPCs add health checks, see below)");
    }

    // Trend, from what this TUI has watched.
    o.push_str(RULE);
    o.push_str("\nFinality trend (watched since this TUI started)\n");
    let h = &snap.history;
    if h.is_empty() {
        o.push_str("  collecting...\n");
    } else {
        let span = h.span(now);
        line(&mut o, "  Watching for", age(span));
        let rate = |secs: i64| {
            let n = h.decided_in(now, secs);
            if span < secs / 2 { format!("{n} (only {} watched)", age(span)) } else { n.to_string() }
        };
        line(&mut o, "  Decided, last 10 min", rate(600));
        line(&mut o, "  Decided, last hour", rate(3_600));
        let n_hour = h.decided_in(now, 3_600);
        if n_hour > 0 {
            line(&mut o, "  Mean time per decision", age(span.min(3_600) / n_hour as i64));
        }
        if let Some(gap_now) = h.gap_at(now, 0) {
            let then = h.gap_at(now, 3_600.min(span));
            let trend = match then {
                Some(g) if gap_now > g + 10 => format!("{gap_now} blocks, up from {g} (falling behind)"),
                Some(g) if g > gap_now + 10 => format!("{gap_now} blocks, down from {g} (catching up)"),
                Some(g) => format!("{gap_now} blocks (was {g})"),
                None => format!("{gap_now} blocks"),
            };
            line(&mut o, "  Finality gap", trend);
        }
        if let Some((spark, max)) = h.gap_sparkline(now, 3_600.min(span.max(60)), 40) {
            let _ = writeln!(o, "  Gap, last {:<5} [{spark}] max {max}", age(3_600.min(span.max(60))));
        }
    }

    // Who is voting.
    o.push_str(RULE);
    let q = snap.quorum.as_array().cloned().unwrap_or_default();
    if !lacks(snap, "get_tfl_quorum_status") && !q.is_empty() {
        o.push_str("\nQuorum (get_tfl_quorum_status)\n");
        let _ = writeln!(o, "  {:<14} {:>20} {:>7} {:<7} {:>6} {:<5} {:<5}", "Finalizer", "Power", "Share", "Online", "Seen", "Prev", "Prec");
        for m in &q {
            let me = if m.get("is_me").and_then(Value::as_bool) == Some(true) { "*" } else { " " };
            let yn = |k: &str| if m.get(k).and_then(Value::as_bool) == Some(true) { "yes" } else { "no" };
            let _ = writeln!(
                o,
                " {me}{:<14} {:>20} {:>6.2}% {:<7} {:>6} {:<5} {:<5}",
                short(s(m, "pub_key"), 10),
                zats(u(m, "voting_power")),
                m.get("power_pct").and_then(Value::as_f64).unwrap_or(0.0),
                yn("online"),
                i(m, "secs_since_seen").map(age).unwrap_or("-".into()),
                yn("prevoted"),
                yn("precommitted"),
            );
        }
    } else {
        recency_quorum(&mut o, snap, now);
    }

    o.push_str(RULE);
    o.push_str("\nRounds at current height (get_tfl_round_diagnosis)\n");
    let rounds = snap.rounds.as_array().cloned().unwrap_or_default();
    if lacks(snap, "get_tfl_round_diagnosis") {
        let _ = writeln!(o, "  {}", lacks_note("get_tfl_round_diagnosis"));
    } else if rounds.is_empty() {
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
    if lacks(snap, "get_tfl_bft_internal_stats") {
        let _ = writeln!(o, "  {}", lacks_note("get_tfl_bft_internal_stats"));
    } else {
        line(&mut o, "  Rounds held / commit cache", format!("{} / {}", opt(u(st, "rounds_data_len")), opt(u(st, "recent_commit_round_cache_len"))));
        line(&mut o, "  BFT blocks / index", format!("{} / {}", opt(u(st, "bft_blocks_len")), opt(u(st, "bft_block_index_len"))));
        line(&mut o, "  BFT peers", st.get("peers").and_then(Value::as_array).map_or("-".into(), |p| p.len().to_string()));
    }
    o
}

/// Per-finalizer votes at the node's BFT height from `get_tfl_recency_status`, with voting power
/// from the roster. Used when the node has no `get_tfl_quorum_status`.
fn recency_quorum(o: &mut String, snap: &Snap, now: i64) {
    o.push_str("\nVotes at this BFT height (get_tfl_recency_status)\n");
    let rows = crate::finality::recent_finalizers(&snap.recency);
    if rows.is_empty() {
        o.push_str("  no finalizers reported (BFT not running yet, or the node is too old for this RPC)\n");
        return;
    }
    let power: std::collections::HashMap<String, u64> = snap
        .roster
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| Some((m.get("pub_key")?.as_str()?.to_ascii_lowercase(), m.get("voting_power")?.as_u64()?)))
        // The roster RPC prints keys in raw byte order; the recency status prints them reversed.
        .flat_map(|(k, p)| [(reverse_hex(&k), p), (k, p)])
        .collect();
    let total: u64 = snap.roster.as_array().into_iter().flatten().filter_map(|m| m.get("voting_power")?.as_u64()).sum();
    let _ = writeln!(o, "  {:<14} {:>7} {:>6} {:>9} {:>9} {:>5}", "Finalizer", "Share", "Seen", "Prevotes", "Precommit", "Round");
    let (mut yes_pre, mut yes_com) = (0u64, 0u64);
    for f in &rows {
        let p = power.get(&f.pub_key.to_ascii_lowercase()).copied();
        if f.prevotes.1 > 0 {
            yes_pre += p.unwrap_or(0);
        }
        if f.precommits.1 > 0 {
            yes_com += p.unwrap_or(0);
        }
        let share = match p {
            Some(p) if total > 0 => format!("{:.2}%", p as f64 * 100.0 / total as f64),
            _ => "-".into(),
        };
        let _ = writeln!(
            o,
            "  {:<14} {:>7} {:>6} {:>4}y/{:<3}n {:>4}y/{:<3}n {:>5}",
            short(&f.pub_key, 10),
            share,
            f.last_direct_utc.map(|t| age(now - t)).unwrap_or("-".into()),
            f.prevotes.1,
            f.prevotes.0,
            f.precommits.1,
            f.precommits.0,
            f.highest_round,
        );
    }
    if total > 0 {
        let need = crate::finality::quorum_threshold(total);
        line(o, "  Yes prevote power", format!("{:.1}% (a decision needs {:.1}%)", yes_pre as f64 * 100.0 / total as f64, need as f64 * 100.0 / total as f64));
        line(o, "  Yes precommit power", format!("{:.1}%", yes_com as f64 * 100.0 / total as f64));
    }
    let _ = writeln!(o, "  Seen = last direct packet from that finalizer. Votes are counted as votes, not by power.");
}

/// The same bytes in the opposite order, as hex.
fn reverse_hex(h: &str) -> String {
    let bytes: Vec<&str> = (0..h.len() / 2).map(|i| &h[2 * i..2 * i + 2]).collect();
    bytes.into_iter().rev().collect()
}

/// One BFT block for the block browser (`get_tfl_bft_block`).
pub fn bft_block_text(b: &Value, tip: Option<u64>) -> String {
    if b.is_null() {
        return "No block at that height.".into();
    }
    let mut o = String::new();
    let h = u(b, "height");
    let pos = match (h, tip) {
        (Some(h), Some(t)) if h == t => " (tip)".to_string(),
        (Some(h), Some(t)) if t > h => format!(" ({} behind the tip)", t - h),
        _ => String::new(),
    };
    line(&mut o, "Height", format!("{}{pos}", opt(h)));
    line(&mut o, "Hash", s(b, "hash"));
    line(&mut o, "Finalizes PoW block", s(b, "candidate_hash"));
    line(&mut o, "Version", opt(u(b, "version")));
    line(&mut o, "Headers / signatures", format!("{} / {}", opt(u(b, "header_count")), opt(u(b, "signature_count"))));
    line(&mut o, "Hardforks carried", opt(u(b, "hardfork_count")));
    let dnib = u(b, "do_not_include_until_bc_height").unwrap_or(0);
    if dnib > 0 {
        line(&mut o, "Not includable before PoW", dnib.to_string());
    }
    o
}

pub fn node_control(snap: &Snap, service: &str) -> String {
    let mut o = String::new();
    if !snap.managed {
        line(&mut o, "Status", status_word(snap));
        o.push_str(RULE);
        let _ = writeln!(
            o,
            "\nNo systemd unit named `{service}` was found{}.\n\
             Start, stop and restart need the node to run as a systemd service (Linux).\n\
             Otherwise start and stop zebrad yourself; every other panel still works over RPC.\n\
             Use --service NAME if the unit has a different name.",
            if cfg!(target_os = "linux") { "" } else { " (systemd is Linux-only)" }
        );
        return o;
    }
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

pub fn version(snap: &Snap, service: &str, config: &str, rpc: &str, argv: &[String], logs: &str) -> String {
    let mut o = String::new();
    line(&mut o, "TUI version", env!("CARGO_PKG_VERSION"));
    line(&mut o, "Node release", crate::upgrade::node_release(snap.node_commit.as_deref(), &snap.info, &snap.chain));
    line(&mut o, "zebrad build", format!("{} (the Zebra version Crosslink is based on)", s(&snap.info, "build")));
    line(&mut o, "zebrad subversion", s(&snap.info, "subversion"));
    line(&mut o, "Protocol version", opt(u(&snap.info, "protocolversion")));
    let tip = u(&snap.chain, "blocks").unwrap_or(0);
    let (up, _) = crate::upgrade::status_line(&snap.info, &snap.chain, tip, snap.avg_block_secs);
    line(&mut o, "Network upgrade", up);
    line(
        &mut o,
        "Staking RPC",
        if crate::upgrade::node_is_v15(&snap.info, &snap.chain) { "wallet_staking_action" } else { "staking_command (pre-v15 node)" },
    );
    line(&mut o, "Chain", s(&snap.chain, "chain"));
    line(&mut o, "Service", service);
    line(&mut o, "Binary", argv.first().cloned().unwrap_or("-".into()));
    line(&mut o, "Config file", config);
    line(&mut o, "RPC", rpc);
    line(&mut o, "Logs", logs);
    line(&mut o, "Unit file", snap.service.get("FragmentPath").cloned().unwrap_or("-".into()));
    line(&mut o, "Drop-ins", snap.service.get("DropInPaths").filter(|d| !d.is_empty()).cloned().unwrap_or("-".into()));
    line(&mut o, "My finalizer", snap.finalizer_address.as_deref().unwrap_or("-"));
    let missing: Vec<&str> = EXTRA_RPCS.iter().copied().filter(|m| lacks(snap, m)).collect();
    line(
        &mut o,
        "Extra RPCs (not upstream)",
        if snap.rpc_error.is_some() {
            "unknown (RPC unreachable)".to_string()
        } else if missing.is_empty() {
            "all present".to_string()
        } else {
            format!("missing {} (see README)", missing.join(", "))
        },
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn top_stakers_marks_you_without_printing_addresses() {
        let mine = "zfinv1_you_do_not_print";
        let other = "zfinv1_other_do_not_print";
        let mut snap = Snap::default();
        snap.finalizer_address = Some(mine.into());
        snap.chain = json!({"blocks": 28000});
        snap.board = json!({
            "milestones": {"roster_snapshot": 34560, "bft_activation": 36288},
            "published_identities": [{
                "public_key": "aa",
                "label": "zk_nd3r",
                "address": other
            }],
            "staking": {
                "complete": true,
                "height": 28000,
                "indexed_from_height": 20736,
                "indexed_through_height": 28000,
                "observed_at": "2026-10-08T02:00:14+00:00",
                "finalizers": [
                    {
                        "public_key": "bb",
                        "finalizer_address": mine,
                        "active_amount_zats": "200000000000",
                        "active_bond_count": 8
                    },
                    {
                        "public_key": "aa",
                        "finalizer_address": other,
                        "active_amount_zats": "100000000000",
                        "active_bond_count": 2
                    }
                ]
            }
        });
        let styled = top_stakers(&snap);
        let text = styled.source().to_string();
        assert!(text.contains("* you"), "{text}");
        assert!(text.contains("zk_nd3r"), "{text}");
        assert!(!text.chars().any(|c| ('\u{2580}'..='\u{259F}').contains(&c)), "{text}");
        // 200 vs 100 against a width of 24: the leader is a solid run of 24, the next row is 12
        // filled and 12 trough. Both runs are spaces, so the ratio does not depend on the font.
        let solid_widths: Vec<usize> = styled
            .spans()
            .filter(|sp| {
                !sp.content.is_empty()
                    && sp.content.chars().all(|c| c == ' ')
                    && sp.attr.color.front == sp.attr.color.back
            })
            .map(|sp| sp.content.chars().count())
            .collect();
        assert!(solid_widths.contains(&24), "{solid_widths:?}");
        assert_eq!(solid_widths.iter().filter(|w| **w == 12).count(), 2, "{solid_widths:?}");
        assert!(!text.contains("zfinv1"), "{text}");
        assert!(!text.contains(mine), "{text}");
        assert!(!text.contains(other), "{text}");
        assert!(text.contains("  1  * you"), "{text}");
        assert!(!text.contains("does not stake"), "{text}");
        assert!(!text.contains("Refreshes"), "{text}");
        assert!(!text.contains("Staking window"), "{text}");
    }

    fn board(observed_at: &str, max_age: i64) -> Value {
        json!({
            "milestones": {"roster_snapshot": 34560, "bft_activation": 36288},
            "staking": {
                "height": 35289,
                "observed_at": observed_at,
                "max_age_seconds": max_age,
                "finalizers": [
                    {"public_key": "0102", "finalizer_address": "zfinv1_a", "active_amount_zats": 300, "active_bond_count": 3},
                    {"public_key": "0304", "finalizer_address": "zfinv1_b", "active_amount_zats": 200, "active_bond_count": 2},
                    {"public_key": "0506", "finalizer_address": "zfinv1_c", "active_amount_zats": 100, "active_bond_count": 1}
                ]
            }
        })
    }

    fn rfc3339_ago(secs: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::seconds(secs)).to_rfc3339()
    }

    #[test]
    fn fresh_board_shows_its_age_and_block() {
        let mut snap = Snap::default();
        snap.chain = json!({"blocks": 35291});
        snap.board = board(&rfc3339_ago(20), 75);
        let age = board_age(&snap);
        assert!(!age.expired, "{}", age.text);
        assert!(age.text.contains("block 35289 (2 behind your node)"), "{}", age.text);
        assert!(top_stakers(&snap).source().contains("valid for 75s"));
    }

    #[test]
    fn board_past_its_limit_is_marked_expired() {
        let mut snap = Snap::default();
        snap.board = board(&rfc3339_ago(600), 75);
        let age = board_age(&snap);
        assert!(age.expired);
        let text = top_stakers(&snap).source().to_string();
        assert!(text.contains("EXPIRED"), "{text}");
        assert!(text.contains("10m ago"), "{text}");
    }

    #[test]
    fn without_a_roster_the_cutoff_is_only_a_projection() {
        let mut snap = Snap::default();
        snap.board = board(&rfc3339_ago(5), 75);
        let text = top_stakers(&snap).source().to_string();
        assert!(text.contains("not committee membership"), "{text}");
        assert!(text.contains("Committee: not formed yet"), "{text}");
        assert!(text.contains("BFT starts at block 36288"), "{text}");
        assert!(!text.contains("Committee from your node"), "{text}");
    }

    #[test]
    fn roster_decides_seats_not_bonded_rank() {
        let mut snap = Snap::default();
        snap.board = board(&rfc3339_ago(5), 75);
        // Seat for b by address, seat for c by its key in the opposite byte order; a has no seat.
        snap.roster = json!([
            {"pub_key": "aaaa", "finalizer_address": "zfinv1_b", "voting_power": 900},
            {"pub_key": "0605", "voting_power": 100}
        ]);
        let text = top_stakers(&snap).source().to_string();
        assert!(text.contains("Committee: 2 seats"), "{text}");
        // A rank cell is three columns wide (`{rank:>3}`) and followed by two spaces.
        // The legend is also indented, so a leading-space search would pick it up.
        let row = |pk: &str| {
            text.lines()
                .find(|l| l.len() >= 5 && &l[3..5] == "  " && l[..3].trim().parse::<u32>().is_ok() && l.contains(pk))
                .unwrap_or("")
                .to_string()
        };
        assert!(row("0102").contains("none"), "{text}");
        assert!(row("0304").contains("#1"), "{text}");
        assert!(row("0506").contains("#2"), "{text}");
        assert!(text.contains("Committee from your node"), "{text}");
        assert!(text.contains("#2 (differs)"), "{text}");
    }

    #[test]
    fn unreachable_node_is_flagged_not_live() {
        let mut snap = Snap::default();
        assert!(node_stale_note(&snap).is_none());
        snap.rpc_error = Some("connection refused".into());
        snap.rpc_ok_at = Some(chrono::Utc::now() - chrono::Duration::seconds(95));
        let note = node_stale_note(&snap).unwrap();
        assert!(note.starts_with("NOT LIVE"), "{note}");
        assert!(note.contains("95s"), "{note}");
        assert!(with_stale_note(&snap, "body".into()).source().ends_with("body"));
    }

    #[test]
    fn retrying_index_is_not_an_empty_board() {
        let mut snap = Snap::default();
        snap.board = json!({
            "staking": {"ok": false, "status": "chain_changed_retrying", "finalizers": null}
        });
        let text = top_stakers(&snap).source().to_string();
        assert!(text.contains("Index updating"), "{text}");
        assert!(!text.contains("No bonds"), "{text}");
    }
}
