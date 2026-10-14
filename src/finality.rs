//! Finality over time, and BFT state from RPCs every node has.
//!
//! The diagnostic RPCs report the present moment. Trends (how often BFT decides, whether finality is
//! falling behind) come from samples this TUI keeps while it runs, so they start empty and fill in.
//! `get_tfl_recency_status` ships with the node release, so the vanilla view below works on any node.

use serde_json::Value;
use std::collections::VecDeque;

/// How far back the history reaches.
pub const KEEP_SECS: i64 = 2 * 3_600;
/// At most one sample per this many seconds, unless the BFT height changed.
const SAMPLE_EVERY: i64 = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub t: i64,
    pub bft_height: u64,
    pub pow_tip: u64,
    pub finalized: Option<u64>,
}

#[derive(Clone, Default)]
pub struct History {
    samples: VecDeque<Sample>,
    /// Times this TUI saw the BFT height go up, with the height reached.
    decisions: VecDeque<(i64, u64)>,
}

impl History {
    pub fn push(&mut self, s: Sample) {
        let advanced = self.samples.back().map(|last| s.bft_height > last.bft_height);
        if advanced == Some(true) {
            self.decisions.push_back((s.t, s.bft_height));
        }
        let due = self.samples.back().is_none_or(|last| s.t - last.t >= SAMPLE_EVERY);
        if due || advanced == Some(true) {
            self.samples.push_back(s);
        }
        while self.samples.front().is_some_and(|f| s.t - f.t > KEEP_SECS) {
            self.samples.pop_front();
        }
        while self.decisions.front().is_some_and(|d| s.t - d.0 > KEEP_SECS) {
            self.decisions.pop_front();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Seconds of history held.
    pub fn span(&self, now: i64) -> i64 {
        self.samples.front().map_or(0, |f| now - f.t)
    }

    /// BFT heights decided in the last `secs` seconds (heights, not observations: a catch-up that
    /// jumps several heights at once counts them all).
    pub fn decided_in(&self, now: i64, secs: i64) -> u64 {
        let first = self.samples.iter().find(|x| now - x.t <= secs);
        match (first, self.samples.back()) {
            (Some(a), Some(b)) => b.bft_height.saturating_sub(a.bft_height),
            _ => 0,
        }
    }

    /// When this TUI last saw the BFT height go up.
    pub fn last_decision_seen(&self) -> Option<i64> {
        self.decisions.back().map(|d| d.0)
    }

    /// When the current BFT height was first seen, which bounds how long it has been stuck.
    pub fn height_since(&self) -> Option<i64> {
        let cur = self.samples.back()?.bft_height;
        self.samples.iter().rev().take_while(|x| x.bft_height == cur).last().map(|x| x.t)
    }

    /// Finality gap (PoW tip minus finalized height) at the sample nearest `secs` ago.
    pub fn gap_at(&self, now: i64, secs: i64) -> Option<u64> {
        let target = now - secs;
        self.samples
            .iter()
            .min_by_key(|x| (x.t - target).abs())
            .and_then(|x| x.finalized.map(|f| x.pow_tip.saturating_sub(f)))
    }

    /// One character per bucket over the last `secs`, scaled to the largest gap shown.
    pub fn gap_sparkline(&self, now: i64, secs: i64, buckets: usize) -> Option<(String, u64)> {
        const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let width = (secs / buckets as i64).max(1);
        let mut vals: Vec<Option<u64>> = vec![None; buckets];
        for x in &self.samples {
            let age = now - x.t;
            if !(0..secs).contains(&age) {
                continue;
            }
            let i = buckets - 1 - (age / width) as usize;
            if let Some(f) = x.finalized {
                let g = x.pow_tip.saturating_sub(f);
                vals[i] = Some(vals[i].map_or(g, |v: u64| v.max(g)));
            }
        }
        let max = vals.iter().flatten().copied().max()?;
        let line = vals
            .iter()
            .map(|v| match v {
                None => ' ',
                Some(_) if max == 0 => BARS[0],
                Some(g) => BARS[((*g as f64 / max as f64) * 7.0).round() as usize],
            })
            .collect();
        Some((line, max))
    }
}

/// One finalizer as `get_tfl_recency_status` reports it.
pub struct Recent {
    pub pub_key: String,
    /// [nil, yes] prevotes and [nil, yes] precommits at the node's current BFT height.
    pub prevotes: (u64, u64),
    pub precommits: (u64, u64),
    pub highest_round: u64,
    pub last_direct_utc: Option<i64>,
}

/// `finalizer_statuses` is a list of `[pub_key, {..}]` pairs.
pub fn recent_finalizers(recency: &Value) -> Vec<Recent> {
    let pair = |v: &Value, i: usize, j: usize| v.get(i).and_then(|x| x.get(j)).and_then(Value::as_u64).unwrap_or(0);
    recency
        .get("finalizer_statuses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let key = entry.get(0)?.as_str()?.to_string();
            let st = entry.get(1)?;
            let votes = st.get("no_yes_votes_in_my_height").cloned().unwrap_or(Value::Null);
            Some(Recent {
                pub_key: key,
                prevotes: (pair(&votes, 0, 0), pair(&votes, 0, 1)),
                precommits: (pair(&votes, 1, 0), pair(&votes, 1, 1)),
                highest_round: st.get("highest_round_vote").and_then(Value::as_u64).unwrap_or(0),
                last_direct_utc: st.get("last_direct_connection_utc").and_then(Value::as_i64).filter(|t| *t > 0),
            })
        })
        .collect()
}

pub fn step_name(step: u64) -> &'static str {
    match step {
        0 => "propose",
        1 => "prevote",
        _ => "precommit",
    }
}

/// Tenderlink's quorum: with `f = (n - 1) / 3` over total power `n`, a decision needs `2f + 1`
/// (or all of `n` when `f` is 0). Matches what the node computes, so the numbers agree.
pub fn quorum_threshold(total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let f = (total - 1) / 3;
    if f == 0 { total } else { 2 * f + 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(t: i64, h: u64, tip: u64, fin: u64) -> Sample {
        Sample { t, bft_height: h, pow_tip: tip, finalized: Some(fin) }
    }

    #[test]
    fn counts_decided_heights_and_stuck_time() {
        let mut h = History::default();
        h.push(s(0, 10, 100, 96));
        h.push(s(10, 11, 101, 97));
        h.push(s(20, 14, 102, 98)); // catch-up: three heights at once
        h.push(s(30, 14, 103, 98));
        h.push(s(40, 14, 104, 98));
        assert_eq!(h.decided_in(40, 60), 4);
        assert_eq!(h.last_decision_seen(), Some(20));
        assert_eq!(h.height_since(), Some(20));
        assert_eq!(h.gap_at(40, 0), Some(6));
    }

    #[test]
    fn old_samples_fall_off() {
        let mut h = History::default();
        h.push(s(0, 1, 10, 6));
        h.push(s(KEEP_SECS + 100, 2, 20, 16));
        assert_eq!(h.span(KEEP_SECS + 100), 0);
    }

    #[test]
    fn reads_recency_pairs() {
        let r = json!({"finalizer_statuses": [["aa", {"no_yes_votes_in_my_height": [[4, 1], [0, 2]], "highest_round_vote": 3, "last_direct_connection_utc": 5}]]});
        let f = recent_finalizers(&r);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].prevotes, (4, 1));
        assert_eq!(f[0].precommits, (0, 2));
        assert_eq!(f[0].highest_round, 3);
        assert_eq!(f[0].last_direct_utc, Some(5));
    }

    #[test]
    fn quorum_matches_tenderlink() {
        assert_eq!(quorum_threshold(0), 0);
        assert_eq!(quorum_threshold(3), 3);
        assert_eq!(quorum_threshold(4), 3);
        assert_eq!(quorum_threshold(12), 7);
    }
}
