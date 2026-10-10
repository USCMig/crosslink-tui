//! Terminal front end for a Crosslink (zebrad) node.
//!
//! It only uses what the node already exposes: JSON-RPC, the systemd unit, the journal and the
//! config file. Nothing in the node is changed.

mod config;
mod node;
mod poll;
mod render;
mod rpc;
mod ui;

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const USAGE: &str = "crosslink-tui [--config PATH] [--rpc URL] [--log-file PATH] [--service NAME] [--read-only]

  --config PATH    zebrad.toml to manage (default: the -c path in the systemd unit's ExecStart,
                   else zebrad's default location for this OS)
  --rpc URL        node JSON-RPC endpoint (default: http://<[rpc] listen_addr>)
  --log-file PATH  read node logs from this file (default on Linux: the unit's journal)
  --service NAME   systemd unit running zebrad, Linux only (default: zebra-crosslink)
  --read-only      watch only: no staking actions, config edits or start/stop/restart, so it is
                   safe next to scripts that manage the node, or pointed at a remote node

Works in any terminal, including tmux:  tmux new -A -s crosslink crosslink-tui";

fn main() {
    let mut service = "zebra-crosslink".to_string();
    let mut config_arg = None;
    let mut rpc_arg = None;
    let mut log_file = None;
    let mut read_only = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--service" => service = args.next().unwrap_or_else(|| usage_exit()),
            "--config" => config_arg = Some(args.next().unwrap_or_else(|| usage_exit())),
            "--rpc" => rpc_arg = Some(args.next().unwrap_or_else(|| usage_exit())),
            "--log-file" => log_file = Some(PathBuf::from(args.next().unwrap_or_else(|| usage_exit()))),
            "--read-only" => read_only = true,
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
    let cache_dir = cfg.get_str(&["state", "cache_dir"]);

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
            rpc::Rpc::new(&rpc_url),
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
            rpc: rpc::Rpc::new(&rpc_url),
            service: service.clone(),
            log_source: log_source.clone(),
            cache_dir: cache_dir.clone(),
            snap: snap.clone(),
            want_logs: want_logs.clone(),
        };
        let sink = siv.cb_sink().clone();
        let stop_poller = stop.clone();
        std::thread::spawn(move || poller.run(stop_poller, move || sink.send(Box::new(ui::refresh)).is_ok()));

        siv.run();
        stop.store(true, Ordering::Relaxed);
        drop(siv);

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
