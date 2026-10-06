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

- A Crosslink `zebrad` node with RPC enabled. The upstream v14 release runs every panel; six of
  the RPCs the TUI can use are extras that upstream v14 does not have (see the next section).
- Linux service control: the node runs as a systemd unit (default name `zebra-crosslink`), and the
  user running the TUI can read its journal (for example, as a member of `adm` or
  `systemd-journal`).

## Node RPCs

These RPCs ship with the upstream v14 release (`ShieldedLabs/crosslink_monolith`, tag `v14`):
`getinfo`, `getblockchaininfo`, `getblockheader`, `getmininginfo`, `getmempoolinfo`, `getpeerinfo`,
`get_tfl_roster_zats`, `get_tfl_final_block_height_and_hash`, `wallet_staking_positions`,
`wallet_spendable_funds` and `staking_command`.

These six are **not part of the upstream v14 release**. The TUI uses them when the node has them, and
otherwise says on screen which one is missing:

| RPC | Used for | Without it |
|---|---|---|
| `get_tfl_finality_status` | Basic Status finality summary; BFT Finality health, heights, quorum power, diagnosis | Shown as not available |
| `get_tfl_quorum_status` | BFT Finality: per-finalizer power, online state and votes | Shown as not available |
| `get_tfl_round_diagnosis` | BFT Finality: proposal and vote state of each round | Shown as not available |
| `get_tfl_bft_internal_stats` | BFT Finality internal stats; BFT peers in Peers and Sync | Shown as not available |
| `get_tfl_bft_block` | BFT Finality: the BFT tip block | Shown as not available |
| `get_wallet_sync_status` | Wallet scan height and balances | Balances come from `wallet_spendable_funds` and `wallet_staking_positions`; the scan height is not shown |

Version Info lists which of the six the connected node is missing.

## Adding the extra RPCs to your node

[`node-patches/v14-extra-rpcs.patch`](node-patches/v14-extra-rpcs.patch) adds all six to a v14
node. They only read state the node already keeps; the patch changes no consensus, networking or
wallet behaviour. It touches six files: `tenderlink/src/lib.rs`, `wallet/src/lib.rs`, and in
`zebra-crosslink/`: `zebra-crosslink/src/lib.rs`, `zebra-rpc/src/methods.rs`,
`zebra-state/src/crosslink.rs` and `zebra-state/src/new_network/bft.rs`.

1. Get the v14 source, or go to your existing v14 checkout:

   ```sh
   git clone https://github.com/ShieldedLabs/crosslink_monolith
   cd crosslink_monolith
   git checkout v14
   ```

2. Apply the patch from the repository root. The check step changes nothing; it only reports
   whether the patch fits:

   ```sh
   git apply --check /path/to/crosslink-tui/node-patches/v14-extra-rpcs.patch
   git apply /path/to/crosslink-tui/node-patches/v14-extra-rpcs.patch
   ```

   If your checkout has its own changes in those files and the check fails, try
   `git apply -3 ...`, which applies what it can and leaves conflict markers for you to resolve.

3. Rebuild the node:

   ```sh
   cd zebra-crosslink
   cargo build --release -p zebrad
   ```

4. Run the new `target/release/zebrad` in place of the old binary, with the same config, and
   restart the node (for example from the TUI's Node Control panel). Your chain state and wallet
   are untouched.

5. Check it worked: Version Info should show `Extra RPCs (not in v14): all present`, or from a shell:

   ```sh
   curl -s -X POST -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","method":"get_tfl_finality_status","params":[],"id":1}' \
     http://127.0.0.1:8232
   ```

The patch is made for v14. A later node release may already include these RPCs, or may need the
patch updated.

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
