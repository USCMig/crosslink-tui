//! Node software version, the scheduled network upgrades, and the node's end of support.
//!
//! v15 brought FeaturenetUpgrade1 at PoW height 55,296: new bonds must be a power of 10 zats, and
//! `staking_command` became `wallet_staking_action`. v17 brings FeaturenetUpgrade2 at 165,000: BFT
//! decides by a strict two-thirds quorum, and fat pointers and carried headers are checked harder.
//! A node that does not know an upgrade is cut off by its peers once the chain reaches it.

use serde_json::Value;

/// A hard fork, with the first release that schedules it and the protocol version that release
/// advertises.
pub struct Fork {
    pub name: &'static str,
    pub height: u64,
    pub release: &'static str,
    pub protocol: u64,
}

/// In activation order.
pub const FORKS: &[Fork] = &[
    Fork { name: "FeaturenetUpgrade1", height: 55_296, release: "v15", protocol: 170_165 },
    Fork { name: "FeaturenetUpgrade2", height: 165_000, release: "v17", protocol: 170_166 },
];

const UPGRADE2: &Fork = &FORKS[1];

/// Blocks before its end of support at which zebrad starts warning, about a week of 75 s blocks.
const EOS_WARN_BLOCKS: u64 = 7 * 1_152;

/// What this node reports about an upgrade, read from `getblockchaininfo.upgrades`.
pub struct Upgrade {
    pub height: u64,
    pub active: bool,
}

pub fn scheduled(chain: &Value, fork: &Fork) -> Option<Upgrade> {
    let ups = chain.get("upgrades")?.as_object()?;
    ups.values().find(|u| u.get("name").and_then(Value::as_str) == Some(fork.name)).map(|u| Upgrade {
        height: u.get("activationheight").and_then(Value::as_u64).unwrap_or(fork.height),
        active: u.get("status").and_then(Value::as_str) == Some("active"),
    })
}

/// True when the node runs a release that knows `fork`: it schedules it or speaks its protocol.
fn knows(info: &Value, chain: &Value, fork: &Fork) -> bool {
    scheduled(chain, fork).is_some() || info.get("protocolversion").and_then(Value::as_u64).is_some_and(|p| p >= fork.protocol)
}

/// True when the node runs v15 or later, so it takes `wallet_staking_action` and power-of-10 bonds.
pub fn node_is_v15(info: &Value, chain: &Value) -> bool {
    knows(info, chain, &FORKS[0])
}

/// Where the chain's own activation height is known, it wins over the built-in one.
fn fork_height(chain: &Value, fork: &Fork) -> u64 {
    scheduled(chain, fork).map_or(fork.height, |u| u.height)
}

/// Whether BFT decides by the strict quorum, floor(2n/3)+1, rather than 2f+1. The roster that votes
/// on the next BFT height is the stake at the PoW block BFT finalized last, so the rule follows the
/// finalized height across FeaturenetUpgrade2.
pub fn strict_quorum(chain: &Value, finalized: Option<u64>) -> bool {
    finalized.is_some_and(|f| f >= fork_height(chain, UPGRADE2))
}

/// Commits of the upstream release tags (`ShieldedLabs/crosslink_monolith`). Where two tags share a
/// commit, the final release is listed first so it wins.
const RELEASES: &[(&str, &str)] = &[
    ("v17_rc1", "97e7f10ef673b14c114cb7c94f5a94986347a429"),
    ("v16", "1dd0b12ddea75dfb0f8f8749b900c9dbb35eb353"),
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

/// The newest release family the node belongs to, from the forks it knows ("v17", "v15"), or
/// `None` when it predates them all.
fn family(info: &Value, chain: &Value) -> Option<&'static str> {
    FORKS.iter().rev().find(|f| knows(info, chain, f)).map(|f| f.release)
}

/// The node's release: by name when its build commit is a release tag, otherwise from what it
/// reports over RPC. The commit comes from the node's startup log, so it is only known when the
/// TUI can read that log.
pub fn node_release(commit: Option<&str>, info: &Value, chain: &Value) -> String {
    let family = match family(info, chain) {
        Some(r) => format!("{r} or later"),
        None => format!("before {}", FORKS[0].release),
    };
    if let Some(c) = commit.filter(|c| c.len() >= 7) {
        if let Some((tag, _)) = RELEASES.iter().find(|(_, full)| full.starts_with(c)) {
            return format!("{tag} (commit {})", &c[..7]);
        }
        return format!("{family}, untagged build (commit {})", &c[..c.len().min(7)]);
    }
    if info.is_null() {
        return "-".into();
    }
    format!("{family} (from its protocol version)")
}

/// Short form for the title bar: the tag alone when known.
pub fn node_release_short(commit: Option<&str>, info: &Value, chain: &Value) -> Option<String> {
    let c = commit.filter(|c| c.len() >= 7)?;
    RELEASES.iter().find(|(_, full)| full.starts_with(c)).map(|(tag, _)| tag.to_string()).or_else(|| {
        Some(match family(info, chain) {
            Some(r) => format!("{r}+"),
            None => format!("pre-{}", FORKS[0].release),
        })
    })
}

/// The release this node must be upgraded to when it lacks a scheduled fork. The newest release
/// covers every fork before it.
pub fn needed_release(info: &Value, chain: &Value) -> Option<&'static str> {
    let newest = FORKS[FORKS.len() - 1].release;
    (!info.is_null() && family(info, chain) != Some(newest)).then_some(newest)
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

fn eta(blocks: u64, avg_block_secs: Option<f64>) -> String {
    match avg_block_secs {
        Some(secs) if secs > 0.0 => {
            let at = chrono::Utc::now() + chrono::Duration::seconds((blocks as f64 * secs) as i64);
            format!(", about {}", at.format("%Y-%m-%d %H:%M UTC"))
        }
        _ => String::new(),
    }
}

/// One line for status panels, and whether it needs attention. It names the next upgrade, or
/// the last one once all are active, and warns when the node lacks one.
pub fn status_line(info: &Value, chain: &Value, tip: u64, avg_block_secs: Option<f64>) -> (String, bool) {
    if info.is_null() {
        return ("-".into(), false);
    }
    for fork in FORKS {
        let height = fork_height(chain, fork);
        let known = knows(info, chain, fork);
        let active = scheduled(chain, fork).is_some_and(|u| u.active) || tip >= height;
        match (known, active) {
            (false, true) => {
                return (
                    format!("! this node predates {} and the network forked at {height}: upgrade the node software", fork.release),
                    true,
                )
            }
            (false, false) => {
                return (
                    format!(
                        "! this node predates {}: upgrade before height {height} ({} blocks{}) or it is cut off",
                        fork.release,
                        height - tip,
                        eta(height - tip, avg_block_secs)
                    ),
                    true,
                )
            }
            (true, false) => {
                return (
                    format!("{} at {height} in {} blocks{} (node is ready)", fork.name, height - tip, eta(height - tip, avg_block_secs)),
                    false,
                )
            }
            (true, true) => {}
        }
    }
    let last = &FORKS[FORKS.len() - 1];
    (format!("{} active since {}", last.name, fork_height(chain, last)), false)
}

/// When this node release stops running, from `getdeprecationinfo`, and whether that is close.
/// zebrad refuses to run past this height, so the node must be upgraded before it.
pub fn end_of_support(deprecation: &Value, tip: u64, avg_block_secs: Option<f64>) -> Option<(String, bool)> {
    let h = deprecation.get("end_of_service")?.get("block_height")?.as_u64()?;
    if tip >= h {
        return Some((format!("! reached at {h}: this release no longer runs, upgrade the node software"), true));
    }
    let left = h - tip;
    let warn = left <= EOS_WARN_BLOCKS;
    Some((format!("{}height {h} ({left} blocks{})", if warn { "! " } else { "" }, eta(left, avg_block_secs)), warn))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chain(forks: &[(&str, u64, &str)]) -> Value {
        let ups: serde_json::Map<String, Value> = forks
            .iter()
            .enumerate()
            .map(|(i, (name, h, st))| (i.to_string(), json!({"name": name, "activationheight": h, "status": st})))
            .collect();
        json!({ "upgrades": ups })
    }

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
        assert_eq!(node_release(Some("1dd0b12d"), &v15.0, &v15.1), "v16 (commit 1dd0b12)");
        assert_eq!(node_release(Some("eca0207c19"), &json!({"protocolversion": 170160}), &json!({})), "v14 (commit eca0207)");
        assert!(node_release(Some("1234567"), &v15.0, &v15.1).starts_with("v15 or later, untagged"));
        assert_eq!(node_release(None, &v15.0, &v15.1), "v15 or later (from its protocol version)");
        assert_eq!(node_release_short(Some("68a076c"), &v15.0, &v15.1).as_deref(), Some("v15"));
        assert_eq!(node_release_short(None, &v15.0, &v15.1), None);
        let v17 = (json!({"protocolversion": 170166}), json!({}));
        assert_eq!(node_release(Some("97e7f10"), &v17.0, &v17.1), "v17_rc1 (commit 97e7f10)");
        assert!(node_release(Some("1234567"), &v17.0, &v17.1).starts_with("v17 or later, untagged"));
        assert_eq!(node_release_short(Some("1234567"), &v17.0, &v17.1).as_deref(), Some("v17+"));
    }

    #[test]
    fn walks_the_upgrades_in_order() {
        let v16 = json!({"protocolversion": 170165});
        let v16_chain = chain(&[("FeaturenetUpgrade1", 55_296, "active")]);
        let (line, warn) = status_line(&v16, &v16_chain, 131_000, None);
        assert!(warn && line.contains("predates v17") && line.contains("165000"), "{line}");
        let (line, warn) = status_line(&v16, &v16_chain, 166_000, None);
        assert!(warn && line.contains("network forked at 165000"), "{line}");
        assert_eq!(needed_release(&v16, &v16_chain), Some("v17"));

        let v17 = json!({"protocolversion": 170166});
        let v17_chain = chain(&[("FeaturenetUpgrade1", 55_296, "active"), ("FeaturenetUpgrade2", 165_000, "pending")]);
        let (line, warn) = status_line(&v17, &v17_chain, 131_000, None);
        assert!(!warn && line.starts_with("FeaturenetUpgrade2 at 165000 in 34000 blocks"), "{line}");
        let (line, warn) = status_line(&v17, &v17_chain, 165_000, None);
        assert!(!warn && line == "FeaturenetUpgrade2 active since 165000", "{line}");
        assert_eq!(needed_release(&v17, &v17_chain), None);

        let v14 = json!({"protocolversion": 170160});
        let (line, warn) = status_line(&v14, &json!({"upgrades": {}}), 50_000, None);
        assert!(warn && line.contains("predates v15"), "{line}");
        assert!(!node_is_v15(&v14, &json!({"upgrades": {}})));
        assert!(node_is_v15(&v14, &v16_chain));
    }

    #[test]
    fn strict_quorum_follows_the_finalized_height() {
        let c = chain(&[("FeaturenetUpgrade2", 165_000, "pending")]);
        assert!(!strict_quorum(&c, None));
        assert!(!strict_quorum(&c, Some(164_999)));
        assert!(strict_quorum(&c, Some(165_000)));
    }

    #[test]
    fn reads_end_of_support() {
        let d = json!({"end_of_service": {"block_height": 188_376, "estimated_time": 1_795_349_488}});
        let (line, warn) = end_of_support(&d, 131_000, None).unwrap();
        assert!(!warn && line == "height 188376 (57376 blocks)", "{line}");
        assert!(end_of_support(&d, 185_000, None).unwrap().1);
        assert!(end_of_support(&d, 188_376, None).unwrap().0.contains("no longer runs"));
        assert!(end_of_support(&json!(null), 1, None).is_none());
    }
}
