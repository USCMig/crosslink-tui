# crosslink-tui

A terminal front end for running a [Crosslink](https://github.com/ShieldedLabs/crosslink_monolith)
`zebrad` node, laid out like [grin](https://github.com/mimblewimble/grin)'s node TUI: a menu on the
left and one panel at a time on the right. It works in any terminal, including tmux.

It does not change the node. Everything goes through what the node already exposes: its JSON-RPC
endpoint, its systemd unit and journal, and its `zebrad.toml`.

## Panels

| Panel | What it does |
|---|---|
| Basic Status | Sync state, peers, disk use, chain tip, BFT finality summary, mempool, wallet balances |
| Peers and Sync | PoW peers (address, direction, agent, last message, ping) and BFT peers |
| Mining | Edit the internal miner's payout address, threads, enabled and low-priority settings; shows running solvers and any GPU solver configured in systemd |
| Staking | Staking window, roster, stake to a roster finalizer or to this node; list your bonds and unbond, withdraw or retarget them; raw `staking_command` editor |
| BFT Finality | `get_tfl_finality_status`, `get_tfl_quorum_status`, `get_tfl_round_diagnosis`, `get_tfl_bft_internal_stats`, `get_tfl_bft_block` |
| Config | Every value in `zebrad.toml`: edit, add, remove, save, reload, or open in `$EDITOR` |
| Node Control | Service state; start, stop and restart with `sudo systemctl`, or a quick restart without sudo |
| Logs | Live journal with filters (warnings, mining, staking/BFT, wallet) |
| Version Info | Build, binary, config, unit and drop-in files, finalizer address |

Config saves keep comments and layout, write a timestamped `.bak` next to the file first, and refuse
to overwrite a file that changed on disk. Staking actions and node start/stop/restart ask for
confirmation, with Cancel selected by default.

## Requirements

- A Crosslink `zebrad` node run by a systemd unit (default name `zebra-crosslink`), with RPC enabled.
  The BFT Finality panel needs a node build that has the `get_tfl_*` diagnostic RPCs; the other
  panels work without them.
- Linux with `systemctl`, `journalctl` and `du`. The user running the TUI needs to read the unit's
  journal (for example, membership of the `adm` or `systemd-journal` group).
- Rust 1.85 or newer to build.

## Build and run

```sh
cargo build --release
tmux new -A -s crosslink ./target/release/crosslink-tui
```

Options:

```
--service NAME  systemd unit running zebrad (default: zebra-crosslink)
--config PATH   zebrad.toml to manage (default: the -c path in the unit's ExecStart,
                else ~/.config/zebrad.toml)
--rpc URL       node JSON-RPC endpoint (default: http://<[rpc] listen_addr>)
```

## Keys

| Key | Action |
|---|---|
| Up/Down | Move through the menu; the panel follows |
| Enter | Go into the panel |
| Tab / arrows | Move between controls |
| Esc | Close a dialog, or go back to the menu |
| Q | Quit |

## Notes

- **Restarts:** Start, Stop and Restart run `sudo systemctl ...`. The TUI steps aside so you can type
  your sudo password, then comes back. Quick restart sends SIGTERM to the node and relies on the
  unit's `Restart=always` to bring it back.
- **Staking window:** the window shown in the Staking panel uses the v14 feature net defaults
  (10368-block period, 3456-block window, first window at height 20736).
- **Finalizer address:** this node's finalizer address is read from the startup line zebrad writes
  to the journal.

## License

MIT. See [LICENSE](LICENSE).
