# crosslink-tui

A terminal front end for running a [Crosslink](https://github.com/ShieldedLabs/crosslink_monolith)
`zebrad` node: a menu on the left and one panel at a time on the right. It works in any terminal,
including tmux, on Linux, macOS and Windows.

It does not change the node. Everything goes through what the node already exposes: its JSON-RPC
endpoint, its `zebrad.toml`, its logs and, on Linux, its systemd unit.

## Panels

| Panel | What it does |
|---|---|
| Basic Status | Sync state, peers, disk use, chain tip, BFT finality summary, mempool, wallet balances |
| Peers and Sync | PoW peers (address, direction, agent, last message, ping) and BFT peers |
| Mining | Edit the internal miner's payout address, threads, enabled and low-priority settings; shows running solvers and any GPU solver configured in systemd |
| Staking | Staking window, roster, stake to a roster finalizer or to this node; list your bonds and unbond, withdraw or retarget them; raw `staking_command` editor |
| BFT Finality | `get_tfl_finality_status`, `get_tfl_quorum_status`, `get_tfl_round_diagnosis`, `get_tfl_bft_internal_stats`, `get_tfl_bft_block` |
| Config | Every value in `zebrad.toml`: edit, add, remove, save, reload, or open in `$EDITOR` |
| Node Control | Service state; start, stop and restart with `sudo systemctl`, or a quick restart without sudo (Linux with systemd) |
| Logs | Live node logs with filters (warnings, mining, staking/BFT, wallet) |
| Version Info | Build, binary, config, unit and drop-in files, finalizer address |

Config saves keep comments and layout, write a timestamped `.bak` next to the file first, and refuse
to overwrite a file that changed on disk. Staking actions and node start/stop/restart ask for
confirmation, with Cancel selected by default.

## Platforms

| | Linux (systemd) | macOS, Windows, Linux without systemd |
|---|---|---|
| Status, peers, mining, staking, BFT, config panels | yes | yes |
| Logs and finalizer address | from the unit's journal, or `--log-file` | from `--log-file` |
| Start, stop, restart | yes | no: start and stop zebrad yourself |

## Install

Download the archive for your platform from the
[releases page](https://github.com/USCMig/crosslink-tui/releases), unpack it, and run
`crosslink-tui` (`crosslink-tui.exe` on Windows). On macOS, if Gatekeeper blocks the unsigned
binary, run `xattr -d com.apple.quarantine crosslink-tui` once.

To build from source instead (Rust 1.85 or newer):

```sh
cargo build --release
./target/release/crosslink-tui
```

## Run

```sh
tmux new -A -s crosslink crosslink-tui                        # Linux, node run by systemd
crosslink-tui --config ./zebrad.toml --log-file ./zebrad.log  # node started by hand
```

Options:

```
--config PATH    zebrad.toml to manage (default: the -c path in the systemd unit's ExecStart,
                 else zebrad's default location for this OS)
--rpc URL        node JSON-RPC endpoint (default: http://<[rpc] listen_addr>)
--log-file PATH  read node logs from this file (default on Linux: the unit's journal)
--service NAME   systemd unit running zebrad, Linux only (default: zebra-crosslink)
```

zebrad's default config locations are `~/.config/zebrad.toml` (Linux),
`~/Library/Preferences/zebrad.toml` (macOS) and `%APPDATA%\zebrad.toml` (Windows). To get a log file
when you start zebrad yourself, redirect its output, for example `zebrad start > zebrad.log 2>&1`.

## Requirements

- A Crosslink `zebrad` node with RPC enabled. The BFT Finality panel needs a node build that has
  the `get_tfl_*` diagnostic RPCs; the other panels work without them.
- Linux service control: the node runs as a systemd unit (default name `zebra-crosslink`), and the
  user running the TUI can read its journal (for example, as a member of `adm` or
  `systemd-journal`).

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
  to its log.

## Releases

Pushing a tag such as `v0.2.0` runs `.github/workflows/release.yml`, which builds Linux (x86_64,
aarch64), macOS (Apple Silicon, Intel) and Windows (x86_64) binaries and attaches them to a GitHub
release for that tag.

## License

MIT. See [LICENSE](LICENSE).
