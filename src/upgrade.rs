//! Node software version and the v15 network upgrade.
//!
//! v15 schedules a hard fork, FeaturenetUpgrade1, at PoW height 55,296. From then on a node still on
//! v14 is cut off by its peers, and new bonds must be a power of 10 zats (the v15 wallet already
//! refuses other amounts). v15 also replaced `staking_command` with `wallet_staking_action`.

use serde_json::Value;

pub const UPGRADE_NAME: &str = "FeaturenetUpgrade1";
pub const UPGRADE_HEIGHT: u64 = 55_296;
/// Network protocol version a v15 node advertises.
pub const V15_PROTOCOL: u64 = 170_165;

/// What this node reports about the upgrade, read from `getblockchaininfo.upgrades`.
pub struct Upgrade {
    pub height: u64,
    pub active: bool,
}

pub fn scheduled(chain: &Value) -> Option<Upgrade> {
    let ups = chain.get("upgrades")?.as_object()?;
    ups.values().find(|u| u.get("name").and_then(Value::as_str) == Some(UPGRADE_NAME)).map(|u| Upgrade {
        height: u.get("activationheight").and_then(Value::as_u64).unwrap_or(UPGRADE_HEIGHT),
        active: u.get("status").and_then(Value::as_str) == Some("active"),
    })
}

/// True when the node runs v15 or later: it schedules the upgrade or speaks its protocol version.
pub fn node_is_v15(info: &Value, chain: &Value) -> bool {
    scheduled(chain).is_some() || info.get("protocolversion").and_then(Value::as_u64).is_some_and(|p| p >= V15_PROTOCOL)
}

/// Commits of the upstream release tags (`ShieldedLabs/crosslink_monolith`). Where two tags share a
/// commit, the final release is listed first so it wins.
const RELEASES: &[(&str, &str)] = &[
    ("v15", "68a076cf3c866c339a89b5684eccea64a4229d7a"),
    ("v14", "eca0207c19469df3abd9da8290bf0aea94da8642"),
    ("v13", "807b995223b6e663cad4cb9890f4ade7dd320295"),
    ("v12", "6d19ec06268ec6f92db0f01e290540032666ef20"),
    ("v12_rc1", "82c5ab383320bbcefffc747217d3381a0fce9bf6"),
    ("s2v11", "2c346b29c06ec19cd5ad4e58752d531888342157"),
    ("s2v10", "b2373576294fc6db1f4254d785c2e5c1a2cf24fe"),
    ("s2v9", "164d7bee065462b627aaa7b030619e82b22f6c75"),
    ("s2v8", "52082e07d8326dda47c69f2994d68e2088aff778"),
];

/// The node's release: by name when its build commit is a release tag, otherwise from what it
/// reports over RPC. The commit comes from the node's startup log, so it is only known when the
/// TUI can read that log.
pub fn node_release(commit: Option<&str>, info: &Value, chain: &Value) -> String {
    if let Some(c) = commit.filter(|c| c.len() >= 7) {
        if let Some((tag, _)) = RELEASES.iter().find(|(_, full)| full.starts_with(c)) {
            return format!("{tag} (commit {})", &c[..7]);
        }
        let family = if node_is_v15(info, chain) { "v15 or later" } else { "before v15" };
        return format!("{family}, untagged build (commit {})", &c[..c.len().min(7)]);
    }
    if info.is_null() {
        return "-".into();
    }
    if node_is_v15(info, chain) { "v15 or later (from its protocol version)".into() } else { "v14 or earlier (from its protocol version)".into() }
}

/// Short form for the title bar: the tag alone when known.
pub fn node_release_short(commit: Option<&str>, info: &Value, chain: &Value) -> Option<String> {
    let c = commit.filter(|c| c.len() >= 7)?;
    RELEASES.iter().find(|(_, full)| full.starts_with(c)).map(|(tag, _)| tag.to_string())
        .or_else(|| Some(if node_is_v15(info, chain) { "v15+".into() } else { "pre-v15".into() }))
}

pub fn is_power_of_10(mut n: u64) -> bool {
    if n == 0 {
        return false;
    }
    while n % 10 == 0 {
        n /= 10;
    }
    n == 1
}

/// The powers of 10 just below and just above `n` (the same value twice when `n` is one).
pub fn nearest_powers_of_10(n: u64) -> (u64, u64) {
    let mut lo = 1u64;
    while lo.checked_mul(10).is_some_and(|x| x <= n) {
        lo *= 10;
    }
    let hi = if lo == n { lo } else { lo.saturating_mul(10) };
    (lo, hi)
}

/// One line for status panels, and whether it needs attention.
pub fn status_line(info: &Value, chain: &Value, tip: u64, avg_block_secs: Option<f64>) -> (String, bool) {
    let eta = |blocks: u64| match avg_block_secs {
        Some(secs) if secs > 0.0 => {
            let at = chrono::Utc::now() + chrono::Duration::seconds((blocks as f64 * secs) as i64);
            format!(", about {}", at.format("%Y-%m-%d %H:%M UTC"))
        }
        _ => String::new(),
    };
    match scheduled(chain) {
        Some(u) if u.active || tip >= u.height => (format!("{UPGRADE_NAME} active since {}", u.height), false),
        Some(u) => (format!("{UPGRADE_NAME} at {} in {} blocks{} (node is ready)", u.height, u.height - tip, eta(u.height - tip)), false),
        None if tip >= UPGRADE_HEIGHT => (
            format!("! this node predates v15 and the network forked at {UPGRADE_HEIGHT}: upgrade the node software"),
            true,
        ),
        None if info.is_null() => ("-".into(), false),
        None => (
            format!(
                "! this node predates v15: upgrade before height {UPGRADE_HEIGHT} ({} blocks{}) or it is cut off",
                UPGRADE_HEIGHT.saturating_sub(tip),
                eta(UPGRADE_HEIGHT.saturating_sub(tip))
            ),
            true,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn powers_of_10() {
        for (n, ok) in [(0, false), (1, true), (10, true), (11, false), (1_000_000_000, true), (64_996_300_000, false)] {
            assert_eq!(is_power_of_10(n), ok, "{n}");
        }
        assert_eq!(nearest_powers_of_10(1_250_000_000), (1_000_000_000, 10_000_000_000));
        assert_eq!(nearest_powers_of_10(100), (100, 100));
        assert_eq!(nearest_powers_of_10(7), (1, 10));
    }

    #[test]
    fn names_the_node_release_from_its_commit() {
        let v15 = (json!({"protocolversion": 170165}), json!({}));
        assert_eq!(node_release(Some("68a076c"), &v15.0, &v15.1), "v15 (commit 68a076c)");
        assert_eq!(node_release(Some("eca0207c19"), &json!({"protocolversion": 170160}), &json!({})), "v14 (commit eca0207)");
        assert!(node_release(Some("1234567"), &v15.0, &v15.1).starts_with("v15 or later, untagged"));
        assert_eq!(node_release(None, &v15.0, &v15.1), "v15 or later (from its protocol version)");
        assert_eq!(node_release_short(Some("68a076c"), &v15.0, &v15.1).as_deref(), Some("v15"));
        assert_eq!(node_release_short(None, &v15.0, &v15.1), None);
    }

    #[test]
    fn reads_the_upgrade_from_blockchaininfo() {
        let chain = json!({"upgrades": {"46545531": {"name": "FeaturenetUpgrade1", "activationheight": 55296, "status": "pending"}}});
        let u = scheduled(&chain).unwrap();
        assert_eq!((u.height, u.active), (55_296, false));
        assert!(node_is_v15(&json!({"protocolversion": 170160}), &chain));
        assert!(!node_is_v15(&json!({"protocolversion": 170160}), &json!({"upgrades": {}})));
        let (line, warn) = status_line(&json!({"protocolversion": 170160}), &json!({"upgrades": {}}), 50_000, None);
        assert!(warn && line.contains("predates v15"), "{line}");
    }
}
