# Isolated Makalu benchmark baseline configuration — 2026-09-30

Status: **prepared and verified, not started; no validator or Lithic workload
benchmark has run.** This is a baseline environment, not a production or Makalu
deployment, gateway activation, or validator acceptance result.

## Source and build pin

- Active Makalu `lithod-mtest-val-02` capture:
  [effective limits and allowlisted settings](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md).
- Exact active executable SHA-256 on both the running validator and isolated
  VPS: `1f03146df86391715b86971b14b6074580b7efd06d7265a1725d90e426b8efbc`.
  Version: `20.0.0`.
- Reproducible preparation script:
  [`scripts/prepare-isolated-makalu-bench.py`](../scripts/prepare-isolated-makalu-bench.py).
  It uses the pinned binary's `init` only to generate default config, overlays
  the allowlisted settings, removes the fresh synthetic keys, and does **not**
  start `lithod`.
- Isolated home: `/var/lib/lithic-bench/makalu-isolated`, owned by the unprivileged
  `lithicbench` account. The preparation script refuses to overwrite it.

## Matched execution/storage settings

The isolated config matches the captured live node for: 100M consensus
`max_gas`, 21M `max_bytes`, GoLevelDB, IAVL cache size 781250, fastnode and lazy
loading disabled, inter-block cache enabled, EVM max-tx-gas-wanted 0, JSON-RPC
gas cap 25M, minimum gas price `0ulitho`, custom pruning 100/10, snapshot
1000/2, blocksync v0, state sync disabled, 900ms commit and 3s proposal
timeouts, and the captured mempool/storage/tx-index settings. The live-only
`iavl-lazy-loading = false` key was inserted into the binary's generated default
config as a narrow overlay; the preparation test covers that case.

## Deliberate isolation and remaining drift

| Area | Isolated setting | Why it differs |
| --- | --- | --- |
| Chain ID | `lithicbench_700778-1` | Cannot join or sign on Makalu `lithosphere_700777-2` |
| P2P | Loopback listener, zero inbound/outbound peers, no seeds or persistent peers, PEX off | No live peers or public network traffic |
| RPC/API | Comet RPC loopback; EVM JSON-RPC, gRPC, gRPC-web and API disabled | No public application listener |
| Keys | No `priv_validator_key.json`, `node_key.json` or `priv_validator_state.json` retained | No validator signing or peer identity |
| Genesis application state | Fresh synthetic state; only chain ID and block limits set | Live chain state was not copied |
| Gateway | Exact **currently running** Makalu binary, not a registered Lithic candidate | Establishes baseline only; native workload needs separately reviewed candidate |

The fresh genesis is **not** byte-identical to Makalu genesis. The active live
binary does not by itself prove the disabled native gateway's performance.
Before full-block/native-state measurements, the candidate binary and native
store lifecycle must be pinned, scalable per-key persistence and shared Go/Rust
fuel metering must be complete, and the Foundation approver must agree the
finite isolated workload envelope. The VPS resize became guest-visible at
13:41 UTC on 2026-09-30: 13 vCPU, 78,193,520 KiB (~74.6 GiB) RAM and five
virtual disks totaling 350 GiB, with a 343 GiB ext4/LVM root filesystem.
Provider confirmation is the only evidence of underlying NVMe backing because
the guest presents QEMU virtual disks. The guest-size mismatch is resolved;
the **350 GiB versus original 500 GB nomination** is a recorded storage-size
variance. Alex subsequently accepted the resized VPS for the isolated
benchmark; this acceptance does not approve any benchmark result or final
production gas/pricing policy.

## Independent destination checks

```text
app.toml     5997f2546f926f7220de9152229e76beb98a1ff06c45d777cbedf22b8bd31350
config.toml  b60ab5d24fbd8fa8569b3b64ddcf8ada8a8dc1908a32050cc01716a2776ac08a
genesis.json b5012a1290c86f837e9eafe40f318e3866f1044eeb54680ac9bd3a4e461ea7e5
```

The three hashes were checked independently after preparation. The benchmark
chain ID is the only `chain_id` in the generated genesis. None of the three
synthetic key files remain, no `lithod start` process exists, and the host has
no new listening ports. Local preparation tests: four Python unit tests pass.

## Post-resize regression smoke checks

After the guest-visible resize, the hash-verified Lithic source archive at
`df2d4fe03cb69ea5d35b628ccbc2822c6d274618` passed
`cargo test --workspace --locked --quiet` on the VPS with Rust 1.96.0. The
native-chain Go module then passed
`go test -mod=mod -tags=lithovm_chain_lab,lithovm_release -count=1 .` with
Go 1.26.0, the existing release FFI library, Evmos lab source
`45d051dc1fe2f0e9e585b09be50c1df80ec99e24`, and SDK lab source
`f2e6295b662fdb27ea33da1296c29588ccdaab42`. The transferred Git archives
were SHA-256 verified before extraction (Evmos
`c1cb7dabe713eaf92ffafcfe49c2af6152c36ac164dee556612194a1b279d0ef`,
SDK `5f27463e32ae2be7cee4b109a9f2e69420af8268d58671557bb17b362581e62a`).
These suites are regression/build
evidence; the Go tests use the current isolated harness and do **not** establish
durable-state throughput, long-lived token affordability, or full-block budgets.

The pinned Evmos lab checkout at
`45d051dc1fe2f0e9e585b09be50c1df80ec99e24` also passed all three keeper
profiles on the same VPS after building the matching pinned debug FFI library:

```text
CGO_ENABLED=1 go test -mod=mod -count=1 ./x/evm/keeper
CGO_ENABLED=1 go test -mod=mod -tags lithovm_chain_lab -count=1 ./x/evm/keeper
CGO_ENABLED=1 go test -mod=mod -race -tags lithovm_chain_lab -run TestKeeperTestSuite/TestLithoVMCandidate -count=1 ./x/evm/keeper
```

All returned `ok` on 2026-09-30. The first tagged attempt failed **before
tests** because the linker expected `target/debug/liblithovm_ffi.so` and only
the release library was present. `cargo build --locked -p lithovm-ffi --quiet`
supplied that library; the rerun passed. This is isolated keeper regression
evidence, not a Makalu broadcast, independent retest, production pricing
approval, or validator-class workload benchmark.

## Subsequent disabled engineering candidate

The later [per-key state and shared live-fuel candidate](NATIVE_PER_KEY_FUEL_CANDIDATE_2026_09_30.md)
is separate from the hash-pinned baseline above. It passed isolated
correctness tests and has not been independently retested or accepted as a
production pricing/benchmark result.
The subsequent [disk-backed token-state run](DURABLE_TOKEN_STATE_BENCH_2026_10_01.md)
measures that later candidate through GoLevelDB/IAVL; it does not run the
configured `lithod` binary or establish validator full-block acceptance.
The [tagged disabled-candidate binary build](DISABLED_CANDIDATE_BINARY_2026_10_01.md)
now clears the lab build prerequisite, but it was not started and is not the
hash-matched live Makalu executable.
