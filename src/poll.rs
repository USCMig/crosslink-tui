//! Background snapshot of node state. The UI only ever renders the latest snapshot, so a slow or
//! dead node never blocks key handling.

use crate::node;
use crate::rpc::Rpc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const POLL_EVERY: Duration = Duration::from_secs(2);
const LOG_LINES: usize = 1500;

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
        let mut last_pid = String::new();
        let lookup_busy = Arc::new(AtomicBool::new(false));
        let found: Arc<Mutex<(Option<String>, Option<String>)>> = Arc::default();
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
        let get = |m: &str, p: Value| self.rpc.call(m, p).unwrap_or(Value::Null);
        s.chain = get("getblockchaininfo", json!([]));
        s.header = match s.chain.get("bestblockhash").and_then(Value::as_str) {
            Some(h) => get("getblockheader", json!([h, true])),
            None => Value::Null,
        };
        s.mining = get("getmininginfo", json!([]));
        s.mempool = get("getmempoolinfo", json!([]));
        s.peers = get("getpeerinfo", json!([]));
        s.wallet = get("get_wallet_sync_status", json!([]));
        s.roster = get("get_tfl_roster_zats", json!([]));
        s.positions = get("wallet_staking_positions", json!([]));
        s.final_tip = get("get_tfl_final_block_height_and_hash", json!([]));
        s.finality = get("get_tfl_finality_status", json!([]));
        s.quorum = get("get_tfl_quorum_status", json!([]));
        s.rounds = get("get_tfl_round_diagnosis", json!([]));
        s.bft_stats = get("get_tfl_bft_internal_stats", json!([]));
        s.bft_block = get("get_tfl_bft_block", json!([]));
    }
}
