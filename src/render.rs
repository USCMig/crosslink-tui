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

/// RPCs this TUI uses that the upstream v14 node release does not have. The README explains how to
/// add them to a node build (node-patches/).
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
    format!("n/a: this node has no {rpc} RPC (not in the upstream v14 release; see the README)")
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
    if lacks(snap, "get_tfl_finality_status") {
        line(&mut o, "BFT Height / Round", lacks_note("get_tfl_finality_status"));
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

/// How many bonded finalizers the snapshot keeps as the voting set.
const COMMITTEE_N: usize = 12;
const BAR_W: usize = 24;
const POOL_W: usize = 40;

struct Seat {
    pk: String,
    label: Option<String>,
    amount: u64,
    bonds: u64,
    me: bool,
}

/// Bonded-stake leaderboard from the public cTAZ roster document.
///
/// The node's `get_tfl_roster_zats` stays empty until the snapshot, so this page reads
/// `staking.finalizers` instead. Finalizer addresses are matched to mark our row and never drawn.
pub fn top_stakers(snap: &Snap) -> StyledString {
    let mut out = StyledString::new();
    let plain = ColorStyle::primary();
    let dim = fg(Color::Light(BaseColor::Black));
    let warn = ColorStyle::title_secondary();
    let green = fg(Color::Light(BaseColor::Green));
    let yellow = fg(Color::Light(BaseColor::Yellow));
    let cyan = fg(Color::Light(BaseColor::Cyan));
    let magenta = fg(Color::Light(BaseColor::Magenta));

    if snap.board.is_null() {
        match &snap.board_error {
            Some(e) => out.append_styled(format!("Board unavailable: {}.\n", brief(e)), warn),
            None => out.append_plain("Fetching...\n"),
        }
        return out;
    }

    let doc = &snap.board;
    let staking = doc.get("staking").cloned().unwrap_or(Value::Null);

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
        let (style, ch) = if i < shown && rows[i].me {
            (green, "█")
        } else if rest > 0 && i + 1 == widths.len() {
            (dim, "░")
        } else {
            (palette.get(i).copied().unwrap_or(dim), "█")
        };
        out.append_styled(ch.repeat(w), style);
    }
    out.append_plain("\n");
    for i in 0..shown {
        let style = if rows[i].me { green } else { palette[i] };
        let name = legend_name(&rows[i]);
        out.append_styled(format!("█ {name} {:.1}%  ", pct(rows[i].amount, total)), style);
    }
    if rest > 0 {
        out.append_styled(format!("░ other {:.1}%", pct(rest, total)), dim);
    }
    out.append_plain("\n");
    out.append_styled(format!("{:>3}  {:<22} {:>14} {:>7} {:>5}\n", "#", "name", "cTAZ", "share", "bonds"), dim);

    for (i, row) in rows.iter().enumerate() {
        let rank = i + 1;
        if i == COMMITTEE_N {
            out.append_styled(format!("── top {COMMITTEE_N} ──\n"), dim);
        }
        let in_set = rank <= COMMITTEE_N;
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
        let filled = blocks(row.amount, leader, BAR_W);
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(BAR_W - filled));
        out.append_styled(
            format!(
                "{rank:>3}  {} {:>14} {:>6.2}% {:>5}  ",
                fit(&seat_name(row), 22),
                ctaz_num(row.amount),
                pct(row.amount, total),
                row.bonds
            ),
            text_style,
        );
        out.append_styled(bar, bar_style);
        out.append_plain("\n");
    }
    append_board_error(&mut out, snap, warn);
    out
}

fn append_board_error(out: &mut StyledString, snap: &Snap, style: ColorStyle) {
    if let Some(e) = &snap.board_error {
        out.append_styled(format!("Last refresh failed: {}. Showing the previous board.\n", brief(e)), style);
    }
}

fn fg(color: Color) -> ColorStyle {
    ColorStyle::new(color, Color::TerminalDefault)
}

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

pub fn bft(snap: &Snap) -> String {
    let mut o = String::new();
    let f = &snap.finality;
    let missing: Vec<&str> = EXTRA_RPCS[..5].iter().copied().filter(|m| lacks(snap, m)).collect();
    if !missing.is_empty() {
        let _ = writeln!(
            o,
            "This node build lacks {} of the 5 BFT diagnostic RPCs this panel uses. They are not part of\n\
             the upstream v14 release; the README's \"Adding the extra RPCs to your node\" section shows\n\
             how to add them.\n{RULE}",
            missing.len()
        );
    }
    o.push_str("Finality status (get_tfl_finality_status)\n");
    if lacks(snap, "get_tfl_finality_status") {
        let _ = writeln!(o, "  {}", lacks_note("get_tfl_finality_status"));
    } else if f.is_null() {
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
    if lacks(snap, "get_tfl_quorum_status") {
        let _ = writeln!(o, "  {}", lacks_note("get_tfl_quorum_status"));
    } else if q.is_empty() {
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
    o.push_str(RULE);
    o.push_str("\nBFT tip block (get_tfl_bft_block)\n");
    let b = &snap.bft_block;
    if lacks(snap, "get_tfl_bft_block") {
        let _ = writeln!(o, "  {}", lacks_note("get_tfl_bft_block"));
    } else if b.is_null() {
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
    line(&mut o, "zebrad build", s(&snap.info, "build"));
    line(&mut o, "zebrad subversion", s(&snap.info, "subversion"));
    line(&mut o, "Protocol version", opt(u(&snap.info, "protocolversion")));
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
        "Extra RPCs (not in v14)",
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
        let text = top_stakers(&snap).source().to_string();
        assert!(text.contains("* you"), "{text}");
        assert!(text.contains("zk_nd3r"), "{text}");
        assert!(text.contains('█'), "{text}");
        assert!(!text.contains("zfinv1"), "{text}");
        assert!(!text.contains(mine), "{text}");
        assert!(!text.contains(other), "{text}");
        assert!(text.contains("  1  * you"), "{text}");
        assert!(!text.contains("does not stake"), "{text}");
        assert!(!text.contains("Refreshes"), "{text}");
        assert!(!text.contains("Staking window"), "{text}");
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
