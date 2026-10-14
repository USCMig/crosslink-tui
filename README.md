# crosslink-tui

A terminal front end for running a [Crosslink](https://github.com/ShieldedLabs/crosslink_monolith)
`zebrad` node: a menu on the left and one panel at a time on the right. It works in any terminal,
including tmux, on Linux, macOS and Windows.

It does not change the node. Everything goes through what the node already exposes: its JSON-RPC
endpoint, its `zebrad.toml`, its logs and, on Linux, its systemd unit.

## Panels

| Panel | What it does |
|---|---|
| Basic Status | Sync state, peers, disk use, chain tip, v15 network upgrade status, BFT finality summary, mempool, wallet balances |
| Peers and Sync | PoW peers (address, direction, agent, last message, ping) and BFT peers |
| Mining | Edit the internal miner's payout address, threads, enabled and low-priority settings; shows running solvers and any GPU solver configured in systemd. Mining health: blocks found in the last hour and day, share of blocks and the Sol/s it implies, and, from the node's logs, accepted and rejected submits, blocks orphaned after acceptance, solutions discarded before submit, and GPU solver failures |
| Staking | Staking window, roster, stake to a roster finalizer or to this node; list your bonds and unbond, withdraw or retarget them; raw staking-action editor. On v15 nodes, enforces power-of-10 bond amounts and offers the nearest valid amounts |
| Top Stakers | Bonded-stake ranking from the public cTAZ roster (`ctaz.cash`), with a bar for each finalizer. Shows how old the ranking is and greys it out once it passes the index's own freshness limit. A `seat` column shows actual committee membership from the node's voting roster, kept separate from bonded rank; until BFT starts the roster is empty and the top-12 line is labelled a projection |
| BFT Finality | Health, BFT height/round/step, finality gap and quorum power; finality trend (decisions per 10 minutes and per hour, time per decision, a finality-gap sparkline); per-finalizer votes; round diagnosis; a block browser for any decided BFT block. Works on any node through `get_tfl_recency_status`; the diagnostic RPCs add health checks, power and round detail |
| Config | Every value in `zebrad.toml`: edit, add, remove, save, reload, or open in `$EDITOR` |
| Node Control | Service state; start, stop and restart with `sudo systemctl`, or a quick restart without sudo (Linux with systemd) |
| Logs | Live node logs with filters (warnings, mining, staking/BFT, wallet) |
| Version Info | Build, protocol version, network upgrade status, which staking RPC the node uses, binary, config, unit and drop-in files, finalizer address |

The header shows the node's status, its BFT height (`BFT STALLED` after 5 minutes without a
decision), and `UPGRADE NODE TO v15` when the node still runs v14.

Config saves keep comments and layout, write a timestamped `.bak` next to the file first, and refuse
to overwrite a file that changed on disk. Staking actions and node start/stop/restart ask for
confirmation, with Cancel selected by default.

## Platforms

| | Linux (systemd) | macOS, Windows, Linux without systemd |
|---|---|---|
| Status, peers, mining, staking, top stakers, BFT, config panels | yes | yes |
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
--read-only      watch only: no staking actions, config edits or start/stop/restart
--miner-address ADDR   payout address whose blocks count as this miner's (default: [mining] miner_address)
```

### Read-only mode

`--read-only` turns the TUI into a monitor. It hides every control that changes something (staking
and bond actions, config edits and saves, `$EDITOR`, start, stop and restart) and refuses those
actions even if one is reached another way. The header shows `READ-ONLY`. Use it to watch a node
that scripts or other tools already manage, or a node on another machine through `--rpc`.

```sh
crosslink-tui --read-only
crosslink-tui --read-only --rpc http://10.0.0.5:8232   # a remote node; logs and service control need it local
```

zebrad's default config locations are `~/.config/zebrad.toml` (Linux),
`~/Library/Preferences/zebrad.toml` (macOS) and `%APPDATA%\zebrad.toml` (Windows). To get a log file
when you start zebrad yourself, redirect its output, for example `zebrad start > zebrad.log 2>&1`.

## Requirements

- A Crosslink `zebrad` node with RPC enabled, v15 or v14. Every panel runs on the upstream releases;
  six of the RPCs the TUI can use are extras the upstream releases do not have (see below).
- Linux service control: the node runs as a systemd unit (default name `zebra-crosslink`), and the
  user running the TUI can read its journal (for example, as a member of `adm` or
  `systemd-journal`).

## Node v15

Node release v15 (`ShieldedLabs/crosslink_monolith`, tag `v15`) schedules a hard fork,
`FeaturenetUpgrade1`, at PoW height 55,296. A node still on v14 after that height is cut off by its
peers. What changes for this TUI:

| v15 change | What the TUI does |
|---|---|
| Hard fork at 55,296 | Basic Status and Version Info show the upgrade and an ETA from recent block times. A v14 node gets `UPGRADE NODE TO v15` in the header |
| `staking_command` is removed; `wallet_staking_action` replaces it and takes the action as a JSON object | Staking actions use `wallet_staking_action` on v15 nodes and `staking_command` on v14 nodes, and fall back to the other one if the node lacks it |
| A new bond must be a power of 10 zats (1, 10, 100, 1000 cTAZ and so on). The v15 wallet refuses other amounts at once, and consensus rejects them from the fork | Staking refuses other amounts and offers the nearest valid ones; Fill max picks the largest that fits |
| New issuance split (90% to bonds, 10% to active finalizers) from the fork | Nothing to change; rewards show up as before |
| BFT fix for a proposer that sends two proposals in one round, which used to stall nodes until restarted | The BFT Finality panel and the header's `BFT STALLED` show whether your node is deciding |

`target_finalizer` in a staking action is the finalizer's `zfinv1...` address. The v15 release notes
show a hex key, but the node accepts only the address.

To upgrade, build v15 (or use the release binary for your platform), stop the node, back up its
cache directory (it holds the chain, the wallet seed and the wallet snapshot), put the v15 binary in
place and start the node with the same config. The cache directory name and the config format did
not change, so the node resumes where it stopped; no resync is needed. Upgrade before height 55,296.

## Node RPCs

These RPCs ship with the upstream releases: `getinfo`, `getblockchaininfo`, `getblockheader`,
`getblockhash`, `getblock`, `getmininginfo`, `getmempoolinfo`, `getpeerinfo`, `get_tfl_roster_zats`,
`get_tfl_final_block_height_and_hash`, `get_tfl_recency_status`, `wallet_staking_positions` and
`wallet_spendable_funds`, plus `wallet_staking_action` (v15) or `staking_command` (v14).

These six are **not part of the upstream releases**. The TUI uses them when the node has them, and
otherwise falls back or says on screen which one is missing:

| RPC | Used for | Without it |
|---|---|---|
| `get_tfl_finality_status` | Finality health, quorum power and diagnosis | Height, round, step and a stall check come from `get_tfl_recency_status` |
| `get_tfl_quorum_status` | Per-finalizer power, online state and votes | A vote table from `get_tfl_recency_status` and the roster |
| `get_tfl_round_diagnosis` | Proposal and vote state of each round | Shown as not available |
| `get_tfl_bft_internal_stats` | BFT internal stats; BFT peers in Peers and Sync | Shown as not available |
| `get_tfl_bft_block` | The BFT block browser | Shown as not available |
| `get_wallet_sync_status` | Wallet scan height and balances | Balances come from `wallet_spendable_funds` and `wallet_staking_positions`; the scan height is not shown |

Version Info lists which of the six the connected node is missing.

## Adding the extra RPCs to your node

Two patches add all six, one per node release:

- [`node-patches/v15-extra-rpcs.patch`](node-patches/v15-extra-rpcs.patch) for v15
- [`node-patches/v14-extra-rpcs.patch`](node-patches/v14-extra-rpcs.patch) for v14

They only read state the node already keeps; neither changes consensus, networking or wallet
behaviour. Each touches six files: `tenderlink/src/lib.rs`, `wallet/src/lib.rs`, and in
`zebra-crosslink/`: `zebra-crosslink/src/lib.rs`, `zebra-rpc/src/methods.rs`,
`zebra-state/src/crosslink.rs` and `zebra-state/src/new_network/bft.rs`.

1. Get the source for your release, or go to your existing checkout:

   ```sh
   git clone https://github.com/ShieldedLabs/crosslink_monolith
   cd crosslink_monolith
   git checkout v15
   ```

2. Apply the matching patch from the repository root. The check step changes nothing; it only
   reports whether the patch fits:

   ```sh
   git apply --check /path/to/crosslink-tui/node-patches/v15-extra-rpcs.patch
   git apply /path/to/crosslink-tui/node-patches/v15-extra-rpcs.patch
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

5. Check it worked: Version Info should show `Extra RPCs (not upstream): all present`, or from a
   shell:

   ```sh
   curl -s -X POST -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","method":"get_tfl_finality_status","params":[],"id":1}' \
     http://127.0.0.1:8232
   ```

A later node release may already include these RPCs, or may need the patch updated.

## Keys

| Key | Action |
|---|---|
| Up/Down | Move through the menu; the panel follows |
| Enter | Go into the panel |
| Tab / arrows | Move between controls |
| Esc | Close a dialog, or go back to the menu |
| Q | Quit |

## Notes

- **Mining health:** blocks found are read from the chain (the coinbase payee of about the last day
  of blocks), so they work against a remote node too; pass `--miner-address` if its config is not
  local. Submit results, orphans, discarded solutions and GPU solver state come from the node's
  logs, so they need the TUI on the node's machine or `--log-file`. zebrad logs every submit as
  "successfully mined a new block" whatever the verdict, so the counts use its `success=` value.
  A few discarded solutions an hour are normal: they are the built-in CPU solver's known bad
  solutions, which the node's own check throws away before submitting.

- **Data age:** if the node's RPC stops answering, the header shows `NOT LIVE` and node panels say
  how long ago their values were last current. The Top Stakers ranking shows the block and time the
  index observed, and is marked `EXPIRED` once it is older than the index's `max_age_seconds`
  (75 seconds today), because a failed refresh keeps the previous list on screen.

- **Restarts:** Start, Stop and Restart run `sudo systemctl ...`. The TUI steps aside so you can type
  your sudo password, then comes back. Quick restart sends SIGTERM to the node and relies on the
  unit's `Restart=always` to bring it back.
- **Staking window:** the window shown in the Staking panel uses the feature net defaults
  (10368-block period, 3456-block window, first window at height 20736); v15 did not change them.
  The window that opens at 51,840 closes at 55,295, the block before the v15 fork.
- **Finality trend:** the trend numbers come from what the TUI has watched since it started, so they
  fill in over the first hour. The block browser reads any decided BFT block by height.
- **Finalizer address:** this node's finalizer address is read from the startup line zebrad writes
  to its log.
- **One outbound request:** the Top Stakers page fetches the public roster document at
  `https://ctaz.cash/v14/api/roster` about every 45 seconds. It is a plain GET that sends no node or
  wallet data, but that site sees your IP address. Every other panel talks only to your own node.

## Releases

Pushing a tag such as `v0.2.0` runs `.github/workflows/release.yml`, which builds Linux (x86_64,
aarch64), macOS (Apple Silicon, Intel) and Windows (x86_64) binaries and attaches them to a GitHub
release for that tag.

## License

MIT. See [LICENSE](LICENSE).
