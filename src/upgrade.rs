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
