//! Terminal front end for a Crosslink (zebrad) node.
//!
//! It only uses what the node already exposes: JSON-RPC, the systemd unit, the journal and the
//! config file. Nothing in the node is changed.

mod config;
mod finality;
mod mining;
mod node;
mod poll;
mod render;
mod rpc;
mod ui;
mod upgrade;

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const USAGE: &str = "crosslink-tui [--config PATH] [--rpc URL] [--rpc-cookie PATH] [--log-file PATH] [--service NAME]
                     [--read-only] [--miner-address ADDR]

  --config PATH    zebrad.toml to manage (default: the -c path in the systemd unit's ExecStart,
                   else zebrad's default location for this OS)
  --rpc URL        node JSON-RPC endpoint (default: http://<[rpc] listen_addr>)
  --rpc-cookie PATH
                   zebrad's RPC auth cookie, needed from v17 unless [rpc] enable_cookie_auth = false
                   (default: <[rpc] cookie_dir>/.cookie, else zebrad's default cache dir, ~/.cache/zebra
                   on Linux)
  --log-file PATH  read node logs from this file (default on Linux: the unit's journal)
  --service NAME   systemd unit running zebrad, Linux only (default: zebra-crosslink)
  --miner-address ADDR
                   payout address whose blocks count as this miner's (default: [mining] miner_address
                   in the config); set it when watching a remote node whose config is elsewhere
  --read-only      watch only: no staking actions, config edits or start/stop/restart, so it is
                   safe next to scripts that manage the node, or pointed at a remote node

Works in any terminal, including tmux:  tmux new -A -s crosslink crosslink-tui";

/// Where a crash report goes: `<cache dir>/crosslink-tui/crash.log`.
fn crash_log_path() -> PathBuf {
    let home = || PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let cache = if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
    } else if cfg!(target_os = "macos") {
        home().join("Library/Caches")
    } else {
        std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".cache"))
    };
    cache.join("crosslink-tui").join("crash.log")
}

/// The UI owns the terminal, so a panic message printed the usual way is lost when the screen is
/// restored. Keep it, append it to the crash log, and print it once the terminal is back.
static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!(
            "{} crosslink-tui {} panicked in thread {thread}: {info}\n{bt}\n",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            env!("CARGO_PKG_VERSION"),
        );
        let path = crash_log_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(text.as_bytes());
        }
        *LAST_PANIC.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{info}"));
    }));
}

fn main() {
    install_panic_hook();
    let mut service = "zebra-crosslink".to_string();
    let mut config_arg = None;
    let mut rpc_arg = None;
    let mut cookie_arg = None;
    let mut log_file = None;
    let mut read_only = false;
    let mut miner_address_arg = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--service" => service = args.next().unwrap_or_else(|| usage_exit()),
            "--config" => config_arg = Some(args.next().unwrap_or_else(|| usage_exit())),
            "--rpc" => rpc_arg = Some(args.next().unwrap_or_else(|| usage_exit())),
            "--rpc-cookie" => cookie_arg = Some(PathBuf::from(args.next().unwrap_or_else(|| usage_exit()))),
            "--log-file" => log_file = Some(PathBuf::from(args.next().unwrap_or_else(|| usage_exit()))),
            "--read-only" => read_only = true,
            "--miner-address" => miner_address_arg = Some(args.next().unwrap_or_else(|| usage_exit())),
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => usage_exit(),
        }
    }

    let props = node::service_props(&service);
    let argv = node::exec_argv(&props);
    let log_source = match log_file {
        Some(p) => node::LogSource::File(p),
        None if node::is_managed(&props) => node::LogSource::Journal(service.clone()),
        None => node::LogSource::None,
    };
    let config_path = config_arg
        .or_else(|| node::config_path_from_argv(&argv))
        .map(PathBuf::from)
        .unwrap_or_else(node::default_config_path);
    let cfg = match config::ConfigFile::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot load the node config: {e}\n\n{USAGE}");
            std::process::exit(1);
        }
    };
    let rpc_url = rpc_arg.unwrap_or_else(|| {
        let addr = cfg.get_str(&["rpc", "listen_addr"]).unwrap_or_else(|| "127.0.0.1:8232".into());
        format!("http://{addr}")
    });
    // With cookie auth off the node writes no cookie, and none is sent.
    let cookie = match cookie_arg {
        Some(p) => Some(p),
        None if cfg.get_bool(&["rpc", "enable_cookie_auth"]) == Some(false) => None,
        None => Some(rpc::default_cookie_path(cfg.get_str(&["rpc", "cookie_dir"]))),
    };
    let cache_dir = cfg.get_str(&["state", "cache_dir"]);
    let miner_address = miner_address_arg.or_else(|| cfg.get_str(&["mining", "miner_address"])).filter(|a| !a.is_empty());

    let snap = Arc::new(Mutex::new(poll::Snap::default()));
    let cfg = Arc::new(Mutex::new(cfg));
    let pending = Arc::new(Mutex::new(None::<ui::Pending>));
    let want_logs = Arc::new(AtomicBool::new(false));
    let panel = Arc::new(Mutex::new("basic".to_string()));

    // Each pass runs the UI until the user quits or asks for a command that needs the terminal
    // (sudo password, $EDITOR); that command runs, then the UI comes back where it was.
    loop {
        let mut siv = cursive::crossterm();
        let app = ui::App::new(
            snap.clone(),
            cfg.clone(),
            rpc::Rpc::new(&rpc_url, cookie.clone()),
            service.clone(),
            argv.clone(),
            log_source.describe(),
            pending.clone(),
            want_logs.clone(),
            panel.clone(),
            read_only,
        );
        ui::build(&mut siv, app);

        let stop = Arc::new(AtomicBool::new(false));
        let poller = poll::Poller {
            rpc: rpc::Rpc::new(&rpc_url, cookie.clone()),
            service: service.clone(),
            log_source: log_source.clone(),
            cache_dir: cache_dir.clone(),
            snap: snap.clone(),
            want_logs: want_logs.clone(),
            miner_address: miner_address.clone(),
        };
        let sink = siv.cb_sink().clone();
        let stop_poller = stop.clone();
        std::thread::spawn(move || poller.run(stop_poller, move || sink.send(Box::new(ui::refresh)).is_ok()));

        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| siv.run()));
        stop.store(true, Ordering::Relaxed);
        drop(siv);
        if ran.is_err() {
            let msg = LAST_PANIC.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
            eprintln!("crosslink-tui crashed: {msg}\nDetails and a backtrace are in {}", crash_log_path().display());
            std::process::exit(101);
        }

        let Some(p) = pending.lock().unwrap().take() else { break };
        println!("$ {}", p.cmd.join(" "));
        match std::process::Command::new(&p.cmd[0]).args(&p.cmd[1..]).status() {
            Ok(st) if st.success() => println!("done."),
            Ok(st) => println!("exited with {st}"),
            Err(e) => println!("could not run it: {e}"),
        }
        if p.reload_config {
            if let Err(e) = cfg.lock().unwrap().reload() {
                println!("config reload failed: {e}");
            }
        }
        print!("Press Enter to return to the TUI...");
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

fn usage_exit() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}
