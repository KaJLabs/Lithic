# Makalu active-node effective limits capture — 2026-09-30

This is a read-only capture from the running Makalu validator, not approval to
change consensus, deploy Lithic contracts, register/activate the native gateway,
or load-test public RPC. No validator/node keys, secrets, process environment,
full TOML or private peer lists were read or copied.

## Method and identity

- Connected with the existing authorized SSH key to the documented `srv02`
  Makalu host. Verified `lithod-mtest-val-02` was active and that local
  `127.0.0.1:26757/status` identified `lithosphere_700777-2`, caught up.
- Ran the reviewed read-only
  [`capture-makalu-effective-limits.sh`](../../scripts/capture-makalu-effective-limits.sh)
  from SSH stdin against service `lithod-mtest-val-02`, home
  `/var/lib/litho-mtest-val-02`, local CometBFT RPC `127.0.0.1:26757`, and
  local EVM RPC `127.0.0.1:8645`. CR bytes introduced by the Windows pipeline
  were stripped before `bash -s`; the successful capture exited 0.
- UTC capture time: `2026-09-30T12:53:06Z`; PID `1093614`.
- A second documented local service, `lithod-mtest-val-03`, was inactive and
  its `127.0.0.1:26857` RPC unavailable, so this is **single-active-node**
  evidence, not an independently cross-checked fleet snapshot.

## Exact observed output

```text
observed_utc=2026-09-30T12:53:06Z
service=lithod-mtest-val-02
pid=1093614
binary_path=/usr/local/bin/lithod-l1-v20.0.0-r1
1f03146df86391715b86971b14b6074580b7efd06d7265a1725d90e426b8efbc  /proc/1093614/exe
20.0.0
d1a11e92f5a9db1dce31963583ee545d53280373825be408ba24002b303c4ca5  /var/lib/litho-mtest-val-02/config/app.toml
0d1405dbc53e0d745238bde1057a898ad87bea0722c7230df14a313505a272d1  /var/lib/litho-mtest-val-02/config/config.toml
a1196fa567400adea3962ea2c0cf24c5d65d07bfca2be7cf6b4dea7cc733935a  /var/lib/litho-mtest-val-02/config/genesis.json
{"network":"lithosphere_700777-2","height":"16237321","block_time":"2026-09-30T12:53:05.494452608Z","catching_up":false}
{"height":"16237322","block":{"max_bytes":"21000000","max_gas":"100000000"}}
{"evm_block":"0xf7c309","evm_gas_limit":"0x5f5e100","evm_block_hash":"0xe8d5450b4a2cd2f0a3165c505923a91103bcf041011774b881132a708c031eee","error":null}
minimum-gas-prices = "0ulitho"
pruning = "custom"
pruning-keep-recent = "100"
pruning-interval = "10"
snapshot-interval = 1000
```

The CometBFT `max_gas` is **100,000,000** and `max_bytes` is **21,000,000** at
the reported height. The EVM header gas limit `0x5f5e100` is also 100,000,000.
These are live observations from one active node, unlike the earlier checked-in
Ansible value; Foundation should still confirm any additional application-side
limit and approve the finite isolated benchmark envelope. The on-disk config
hashes identify the files inspected but do not by themselves prove every
execution-relevant setting currently loaded in memory.

## Isolated-host binary pin

The on-disk active binary was separately hashed at its resolved path and matched
`/proc/1093614/exe` byte-for-byte by SHA-256. Only that 141,221,080-byte
executable was transferred to the dedicated Lithic benchmark VPS at
`/var/lib/lithic-bench/source/lithod-l1-v20.0.0-r1`. The destination SHA-256
was independently checked as
`1f03146df86391715b86971b14b6074580b7efd06d7265a1725d90e426b8efbc`,
and `version` returned `20.0.0`. The binary was **not** started as a node; no
validator keys, raw config, chain data, or live peers were transferred.

## Allowlisted execution/storage settings

The following values were parsed on the active host from the hash-pinned config
files. Only these non-secret keys were returned; no full TOML or peer/key fields
were exported. The `genesis.json` block values agree with the live RPC response.

| File | Setting | Observed value |
| --- | --- | --- |
| `app.toml` | `app-db-backend` | empty string (application default) |
| `app.toml` | `evm.max-tx-gas-wanted` | `0` |
| `app.toml` | `iavl-cache-size` | `781250` |
| `app.toml` | `iavl-disable-fastnode` / `iavl-lazy-loading` | `false` / `false` |
| `app.toml` | `inter-block-cache` | `true` |
| `app.toml` | `json-rpc.gas-cap` | `25000000` |
| `app.toml` | `minimum-gas-prices` | `0ulitho` |
| `app.toml` | `pruning` / `pruning-keep-recent` / `pruning-interval` | `custom` / `100` / `10` |
| `app.toml` | `state-sync.snapshot-interval` / `state-sync.snapshot-keep-recent` | `1000` / `2` |
| `config.toml` | `db_backend` | `goleveldb` |
| `config.toml` | `blocksync.version` / `statesync.enable` | `v0` / `false` |
| `config.toml` | `consensus.create_empty_blocks` / `timeout_commit` / `timeout_propose` | `true` / `900ms` / `3s` |
| `config.toml` | `mempool.max_tx_bytes` / `max_txs_bytes` / `size` | `1048576` / `1073741824` / `5000` |
| `config.toml` | `storage.discard_abci_responses` / `tx_index.indexer` | `false` / `kv` |
| `genesis.json` | `chain_id` / `consensus.params.block.max_gas` / `max_bytes` | `lithosphere_700777-2` / `100000000` / `21000000` |

These settings are a benchmark comparison baseline, not a copy-ready config.
An isolated node must use a distinct chain identity, no live peers/signing
duties, and loopback-only RPC/P2P; any such deliberate isolation differences
must be recorded separately from execution-config drift.
