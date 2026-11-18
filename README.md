# crosslink-tui

A terminal front end for running a [Crosslink](https://github.com/ShieldedLabs/crosslink_monolith)
`zebrad` node: a menu on the left and one panel at a time on the right. It works in any terminal,
including tmux, on Linux, macOS and Windows.

It does not change the node. Everything goes through what the node already exposes: its JSON-RPC
endpoint, its `zebrad.toml`, its logs and, on Linux, its systemd unit.

## Panels

| Panel | What it does |
|---|---|
| Basic Status | Node release (for example v18), sync state, peers, disk use, chain tip, the FeaturenetUpgrade3 countdown, the height at which this node release stops running, BFT finality summary, mempool, wallet balances |
| Peers and Sync | Peers on the v18 network with their finalized and tip heights next to yours, how many are at your finalized height, behind or ahead; legacy Zcash peers if that network is on; BFT peers |
| Mining | Edit the internal miner's payout address, threads, enabled and low-priority settings; shows running solvers and any GPU solver configured in systemd. Mining health: blocks found in the last hour and day, share of blocks and the Sol/s it implies, and, from the node's logs, accepted and rejected submits, blocks orphaned after acceptance, solutions discarded before submit, and GPU solver failures |
| Staking | Staking window, roster with each finalizer's address (yours marked), stake to a roster finalizer or to this node; list your bonds and unbond, withdraw or retarget them; raw staking-action editor. Enforces power-of-10 bond amounts and offers the nearest valid amounts |
| Top Stakers | Bonded-stake ranking from the public cTAZ roster (`ctaz.cash`), with a bar for each finalizer. Shows how old the ranking is and greys it out once it passes the index's own freshness limit. A `seat` column shows actual committee membership from the node's voting roster, kept separate from bonded rank |
| BFT Finality | Health, BFT height/round/step, finality gap and quorum power; finality trend (decisions per 10 minutes and per hour, time per decision, a finality-gap sparkline); per-finalizer votes; round diagnosis; a block browser for any decided BFT block. Works on any node through `get_tfl_recency_status`; the diagnostic RPCs add health checks, power and round detail |
| Config | Every value in `zebrad.toml`: edit, add, remove, save, reload, or open in `$EDITOR` |
| Node Control | Service state; start, stop and restart with `sudo systemctl`, or a quick restart without sudo (Linux with systemd) |
| Logs | Live node logs with filters (warnings, mining, staking/BFT, wallet) |
| Version Info | Node release and the Zebra version it is based on, protocol version, network upgrade status, end of support, binary, config, unit and drop-in files, finalizer address, which extra RPCs the node has |

The header shows the node's release (for example `Crosslink Node v18`), its status and its BFT
height (`BFT STALLED` after 5 minutes without a decision). It also shows `UPGRADE NODE TO v18` when
the node is not on v18, and `NODE RELEASE NEAR END OF SUPPORT` in the week before the node release
stops running.

Config saves keep comments and layout, write a timestamped `.bak` next to the file first, and refuse
to overwrite a file that changed on disk. Staking actions and node start/stop/restart ask for
confirmation, with Cancel selected by default.

## Node v18

The TUI targets node release v18 (`ShieldedLabs/crosslink_monolith`, tag `v18`). What matters on
v18:

| | What it means | What the TUI does |
|---|---|---|
| New peer-to-peer network | A v18 node cannot talk to a v17 one. PoW sync, BFT votes and transactions all travel over one UDP port | Peers and Sync lists the v18 peers and compares their heights with yours |
| One network port | `[state] network_local_port` sets the port, and `0` picks a random one on every start. `[network] public_address` now supplies only the IP to advertise. Pin the port if your router forwards one | Nothing; edit it in Config |
| Legacy Zcash network off by default | `getpeerinfo` lists nothing and `getinfo` reports 0 connections | Connected Peers counts v18 peers instead. v18 has no RPC for them, so the TUI reads the status line the node logs every few seconds (systemd journal or `--log-file`) |
| Config fields removed | `[crosslink]` rejects unknown fields, so a node whose config still has `disable_zaino` does not start. `bft_peers` is ignored: BFT finds its peers over the new network | Delete stale fields in Config |
| Hard fork `FeaturenetUpgrade3` at PoW height 189,000 | Nodes not on v18 are cut off at the fork. From it, a staking action on an existing bond names the bond's current version (the txid of the last transaction that created or acted on it); the node's wallet fills that in | Basic Status and Version Info show the fork height with an ETA from recent block times. Staking actions are unchanged |
| End of support at height 212,568 | v18 refuses to run past it, so upgrade to the next release before then | Basic Status and Version Info show the height and an ETA (from `getdeprecationinfo`); the header warns in the last week |
| `get_tfl_roster_with_addresses` | New RPC: the voting roster with each finalizer's `zfinv1...` address, which `get_tfl_roster_zats` leaves empty | The Staking roster uses it, so you can stake to any roster finalizer |
| RPC cookie auth is on by default (since v17) | Every RPC without zebrad's cookie is refused with HTTP 401 | The TUI sends the cookie (see below) |
| Strict BFT quorum (since FeaturenetUpgrade2) | A decision needs more than two thirds of the active stake, `floor(2n/3)+1`. Finality stalls until finalizers holding that much stake run a release that can talk to each other | The vote table and the extra-RPC patch compute the quorum the way the node does |
| Bonds must be a power of 10 zats | 1, 10, 100, 1000 cTAZ and so on. Unbonding and withdrawing take any amount | Staking refuses other amounts and offers the nearest valid ones; Fill max picks the largest that fits |
| Staking RPC is `wallet_staking_action` | It takes the action as a JSON object; `target_finalizer` is the finalizer's `zfinv1...` address | Staking actions use it |

### RPC cookie

With cookie auth on, zebrad writes a new cookie each time it starts, to `.cookie` in `[rpc]
cookie_dir`. That defaults to zebrad's cache directory, `~/.cache/zebra` on Linux, which is not
`[state] cache_dir`. The TUI reads the cookie again on every call, so a node restart does not break
it. `--rpc-cookie PATH` points it elsewhere, for example at a cookie copied from another machine.
If the node refuses the cookie, or there is none, the status line says which.

With `[rpc] enable_cookie_auth = false` in the node config, the TUI sends no cookie. Your own
scripts that call the RPC need the cookie when auth is on, for example:

```sh
curl -s -u "$(cat ~/.cache/zebra/.cookie)" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' http://127.0.0.1:8232
```

### Upgrading the node from v17

1. Build v18 from source (tag `v18`), or use the release binary for your platform.
2. Stop the node and back up its cache directory. It holds the chain, the wallet seed and the wallet
   snapshot.
3. In the config, delete `disable_zaino` from `[crosslink]` if it is there, and set
   `[state] network_local_port` to the port your router forwards (the port of `public_address`).
4. Put the v18 binary in place and start the node. It resumes from the same cache directory; no
   resync is needed. Do not go back to v17 on that cache directory afterwards: per the v18 release,
   that is not supported.

Finality waits until finalizers holding more than two thirds of the stake have upgraded, while PoW
blocks keep coming. BFT Finality shows who is voting.

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
--rpc-cookie PATH
                 zebrad's RPC auth cookie (default: <[rpc] cookie_dir>/.cookie, else
                 ~/.cache/zebra/.cookie on Linux; not used when enable_cookie_auth = false)
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
crosslink-tui --read-only --rpc http://10.0.0.5:8232 --rpc-cookie ./node.cookie   # a remote node
```

A remote node needs `[rpc] listen_addr` reachable from this machine, and its cookie copied over when
cookie auth is on. Logs and service control need the TUI on the node's machine.

zebrad's default config locations are `~/.config/zebrad.toml` (Linux),
`~/Library/Preferences/zebrad.toml` (macOS) and `%APPDATA%\zebrad.toml` (Windows). To get a log file
when you start zebrad yourself, redirect its output, for example `zebrad start > zebrad.log 2>&1`.

## Requirements

- A Crosslink `zebrad` v18 node with RPC enabled. Every panel runs on the upstream release; six of
  the RPCs the TUI can use are extras the upstream release does not have (see below).
- Linux service control: the node runs as a systemd unit (default name `zebra-crosslink`), and the
  user running the TUI can read its journal (for example, as a member of `adm` or
  `systemd-journal`).

## Node RPCs

These RPCs ship with v18: `getinfo`, `getblockchaininfo`, `getblockheader`, `getblockhash`,
`getblock`, `getmininginfo`, `getmempoolinfo`, `getpeerinfo`, `getdeprecationinfo`,
`get_tfl_roster_with_addresses` (`get_tfl_roster_zats` on older nodes), `get_tfl_final_block_height_and_hash`, `get_tfl_recency_status`,
`wallet_staking_positions`, `wallet_spendable_funds` and `wallet_staking_action`.

These six are **not part of the upstream release**. The TUI uses them when the node has them, and
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

[`node-patches/v18-extra-rpcs.patch`](node-patches/v18-extra-rpcs.patch) adds all six to a v18
node (`v17-extra-rpcs.patch` is kept for v17 nodes). It only reads state the node already keeps
and changes no consensus, networking or wallet behaviour. Its quorum figures follow the strict
quorum from FeaturenetUpgrade2, as consensus does.
It touches six files: `tenderlink/src/lib.rs`, `wallet/src/lib.rs`, and in `zebra-crosslink/`:
`zebra-crosslink/src/lib.rs`, `zebra-rpc/src/methods.rs`, `zebra-state/src/crosslink.rs` and
`zebra-state/src/new_network/bft.rs`.

1. Get the v18 source, or go to your existing checkout:

   ```sh
   git clone https://github.com/ShieldedLabs/crosslink_monolith
   cd crosslink_monolith
   git checkout v18
   ```

2. Apply the patch from the repository root. The check step changes nothing; it only reports
   whether the patch fits:

   ```sh
   git apply --check /path/to/crosslink-tui/node-patches/v18-extra-rpcs.patch
   git apply /path/to/crosslink-tui/node-patches/v18-extra-rpcs.patch
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
   shell (drop `-u ...` if cookie auth is off):

   ```sh
   curl -s -u "$(cat ~/.cache/zebra/.cookie)" -H 'Content-Type: application/json' \
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
- **Staking window:** the window shown in the Staking panel uses the feature net defaults: a
  10368-block period with a 3456-block window at its start.
- **Finality trend:** the trend numbers come from what the TUI has watched since it started, so they
  fill in over the first hour. The block browser reads any decided BFT block by height.
- **Startup facts:** the finalizer address, solver count and build commit come from lines zebrad
  writes to its log at startup. With systemd, the TUI reads the journal forward from the moment the
  node started, which stays fast however busy the journal is.
- **Node release:** when the build commit is an upstream release tag, the TUI names the release
  (`v18 (commit 8ea914c)`); a build with local patches on top of a tag still reports the tag's
  commit. Without the log (a remote node), the release is inferred from the protocol version and the
  upgrades the node schedules (`v18 or later`). `zebrad build` (`v6.2.3`) is the Zebra version
  Crosslink is based on, not the Crosslink release.
- **Network peers:** v18 has no RPC for its peers, so Peers and Sync reads the status line the node
  logs every few seconds. Like the startup facts, it needs the TUI on the node's machine or
  `--log-file`; against a remote node it shows none. Peers are compared by height only: a peer at
  your finalized height has not been checked to hold the same block, but with finality working
  every honest node agrees on the finalized blocks.
- **One outbound request:** the Top Stakers page fetches the public roster document at
  `https://ctaz.cash/v14/api/roster` about every 45 seconds. It is a plain GET that sends no node or
  wallet data, but that site sees your IP address. Every other panel talks only to your own node.

## Releases

Pushing a tag such as `v0.5.1` runs `.github/workflows/release.yml`, which builds Linux (x86_64,
aarch64), macOS (Apple Silicon, Intel) and Windows (x86_64) binaries and attaches them to a GitHub
release for that tag.

## License

MIT. See [LICENSE](LICENSE).
