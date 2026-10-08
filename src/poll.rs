//! Background snapshot of node state. The UI only ever renders the latest snapshot, so a slow or
//! dead node never blocks key handling.

use crate::node;
use crate::rpc::Rpc;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const POLL_EVERY: Duration = Duration::from_secs(2);
const LOG_LINES: usize = 1500;
/// Public bonded-stake index. The node roster RPC stays empty until the snapshot.
const ROSTER_URL: &str = "https://ctaz.cash/v14/api/roster";
const BOARD_EVERY: Duration = Duration::from_secs(45);

#[derive(Default, Clone)]
pub struct Snap {
    pub rpc_error: Option<String>,
    pub info: Value,
    pub chain: Value,
    pub header: Value,
    pub mining: Value,
    pub mempool: Value,
    pub peers: Value,
    pub wallet: Value,
    pub roster: Value,
    /// `ctaz.cash` roster document. `staking.finalizers` is the bonded-stake board.
    pub board: Value,
    /// Set when the latest board fetch failed. The previous `board` is kept.
    pub board_error: Option<String>,
    pub final_tip: Value,
    pub finality: Value,
    pub quorum: Value,
    pub rounds: Value,
    pub bft_stats: Value,
    pub bft_block: Value,
    pub service: HashMap<String, String>,
    /// A service manager (systemd) runs the node, so start/stop/status apply.
    pub managed: bool,
    pub logs: Vec<String>,
    pub disk_bytes: Option<u64>,
    pub finalizer_address: Option<String>,
    pub solvers: Option<String>,
    pub positions: Value,
    /// RPCs this node build answered with "Method not found".
    pub missing_rpcs: BTreeSet<String>,
    /// Balances came from `wallet_spendable_funds` because `get_wallet_sync_status` is missing.
    pub wallet_fallback: bool,
    pub updated: Option<chrono::DateTime<chrono::Local>>,
}

impl Snap {
    pub fn node_active(&self) -> bool {
        self.service.get("ActiveState").is_some_and(|s| s == "active")
    }
}

pub struct Poller {
    pub rpc: Rpc,
    pub service: String,
    pub log_source: node::LogSource,
    pub cache_dir: Option<String>,
    pub snap: Arc<Mutex<Snap>>,
    pub want_logs: Arc<AtomicBool>,
}

impl Poller {
    /// Runs until `stop` is set or the UI goes away (`notify` returns false).
    pub fn run(self, stop: Arc<AtomicBool>, notify: impl Fn() -> bool) {
        let mut last_disk = None::<Instant>;
        let mut last_lookup = None::<Instant>;
        let mut last_board = None::<Instant>;
        let mut last_pid = String::new();
        let lookup_busy = Arc::new(AtomicBool::new(false));
        let board_busy = Arc::new(AtomicBool::new(false));
        let found: Arc<Mutex<(Option<String>, Option<String>)>> = Arc::default();
        let board_slot: Arc<Mutex<Option<Result<Value, String>>>> = Arc::default();
        while !stop.load(Ordering::Relaxed) {
            let mut s = self.snap.lock().unwrap().clone();
            s.service = node::service_props(&self.service);
            s.managed = node::is_managed(&s.service);

            if last_disk.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                if let Some(dir) = &self.cache_dir {
                    s.disk_bytes = node::disk_usage_bytes(dir);
                }
                last_disk = Some(Instant::now());
            }
            // Both are logged once at startup and a journal search takes seconds, so look them up
            // off the poll loop, and only when the node process changes (or, while missing, once a
            // minute).
            let pid = s.service.get("MainPID").cloned().unwrap_or_default();
            let missing = s.finalizer_address.is_none() || s.solvers.is_none();
            let due = pid != last_pid || (missing && last_lookup.is_none_or(|t| t.elapsed() > Duration::from_secs(60)));
            if pid != last_pid {
                // A restart may be a different build, so probe every RPC again.
                s.missing_rpcs.clear();
            }
            if due && !lookup_busy.swap(true, Ordering::Relaxed) {
                last_pid = pid;
                last_lookup = Some(Instant::now());
                let (src, found, busy) = (self.log_source.clone(), found.clone(), lookup_busy.clone());
                std::thread::spawn(move || {
                    let r = (node::finalizer_address(&src), node::running_solvers(&src));
                    *found.lock().unwrap() = r;
                    busy.store(false, Ordering::Relaxed);
                });
            }
            {
                let (addr, solvers) = &*found.lock().unwrap();
                if addr.is_some() {
                    s.finalizer_address = addr.clone();
                }
                s.solvers = solvers.clone();
            }

            // The board is a public HTTP document, so fetch it off this loop (a slow TLS
            // call must not stall the 2s refresh) and even when the node RPC is down.
            if let Some(result) = board_slot.lock().unwrap().take() {
                match result {
                    Ok(v) => {
                        s.board = v;
                        s.board_error = None;
                    }
                    Err(e) => s.board_error = Some(e),
                }
            }
            if last_board.is_none_or(|t| t.elapsed() > BOARD_EVERY) && !board_busy.swap(true, Ordering::Relaxed) {
                last_board = Some(Instant::now());
                let (slot, busy) = (board_slot.clone(), board_busy.clone());
                std::thread::spawn(move || {
                    struct Release(Arc<AtomicBool>);
                    impl Drop for Release {
                        fn drop(&mut self) {
                            self.0.store(false, Ordering::Relaxed);
                        }
                    }
                    let _release = Release(busy);
                    let result = crate::rpc::get_json(ROSTER_URL, Duration::from_secs(8));
                    *slot.lock().unwrap() = Some(result);
                });
            }

            self.fetch_rpc(&mut s);
            if self.want_logs.load(Ordering::Relaxed) {
                s.logs = node::log_lines(&self.log_source, LOG_LINES);
            }
            s.updated = Some(chrono::Local::now());
            *self.snap.lock().unwrap() = s;
            if !notify() {
                return;
            }
            std::thread::sleep(POLL_EVERY);
        }
    }

    fn fetch_rpc(&self, s: &mut Snap) {
        // One cheap probe first: if the node is down, do not wait out a dozen timeouts.
        match self.rpc.call("getinfo", json!([])) {
            Ok(v) => {
                s.info = v;
                s.rpc_error = None;
            }
            Err(e) => {
                s.rpc_error = Some(e);
                return;
            }
        }
        s.chain = self.get(s, "getblockchaininfo", json!([]));
        s.header = match s.chain.get("bestblockhash").and_then(Value::as_str).map(String::from) {
            Some(h) => self.get(s, "getblockheader", json!([h, true])),
            None => Value::Null,
        };
        s.mining = self.get(s, "getmininginfo", json!([]));
        s.mempool = self.get(s, "getmempoolinfo", json!([]));
        s.peers = self.get(s, "getpeerinfo", json!([]));
        s.roster = self.get(s, "get_tfl_roster_zats", json!([]));
        s.positions = self.get(s, "wallet_staking_positions", json!([]));
        s.wallet = self.get(s, "get_wallet_sync_status", json!([]));
        s.wallet_fallback = s.missing_rpcs.contains("get_wallet_sync_status");
        if s.wallet_fallback {
            s.wallet = self.wallet_from_vanilla_rpcs(s);
        }
        s.final_tip = self.get(s, "get_tfl_final_block_height_and_hash", json!([]));
        s.finality = self.get(s, "get_tfl_finality_status", json!([]));
        s.quorum = self.get(s, "get_tfl_quorum_status", json!([]));
        s.rounds = self.get(s, "get_tfl_round_diagnosis", json!([]));
        s.bft_stats = self.get(s, "get_tfl_bft_internal_stats", json!([]));
        s.bft_block = self.get(s, "get_tfl_bft_block", json!([]));
    }

    /// Calls `method`, remembering it if this node build does not have it.
    fn get(&self, s: &mut Snap, method: &str, params: Value) -> Value {
        if s.missing_rpcs.contains(method) {
            return Value::Null;
        }
        match self.rpc.call(method, params) {
            Ok(v) => v,
            Err(e) => {
                if e.contains("Method not found") {
                    s.missing_rpcs.insert(method.to_string());
                }
                Value::Null
            }
        }
    }

    /// The `get_wallet_sync_status` fields that vanilla RPCs can supply: balances from
    /// `wallet_spendable_funds`, staked and withdrawable totals from `wallet_staking_positions`.
    /// There is no vanilla equivalent of the wallet's scan height.
    fn wallet_from_vanilla_rpcs(&self, s: &mut Snap) -> Value {
        let funds = self.get(s, "wallet_spendable_funds", json!([]));
        if funds.is_null() {
            return Value::Null;
        }
        let total = |list: Option<&Value>| -> u64 {
            list.and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|b| b.get("latest_val").and_then(Value::as_u64))
                .sum()
        };
        let staked: u64 = s
            .positions
            .get("active")
            .and_then(Value::as_object)
            .map(|m| m.values().map(|v| total(Some(v))).sum())
            .unwrap_or(0);
        json!({
            "tip_height": funds.get("tip_height"),
            "user_shielded_spendable_zats": funds.get("spendable_zats"),
            "user_shielded_pending_zats": funds.get("pending_zats"),
            "user_unshielded_zats": funds.get("unshielded_zats"),
            "staked_zats": staked,
            "withdrawable_zats": total(s.positions.get("withdrawable")),
        })
    }
}
