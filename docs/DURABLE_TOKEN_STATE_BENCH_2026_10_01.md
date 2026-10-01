# Disk-backed token-state benchmark — 2026-10-01

Status: **disabled engineering evidence, not validator acceptance or production
pricing**. No live Makalu process, gateway, contract or LAX state was changed.
Alex accepted the isolated 13-vCPU/78-GB/350-GB VPS as the benchmark host.
This run uses GoLevelDB/IAVL on that host but calls the native FFI through the
isolated `StateDB` harness, not the full `lithod` block executor.

The harness and fixture are Lithic commit
`c28cf64f83be25b1ae2ca9a27b3ae77f4f27fc73`. The full run used its
isolated working copy; `integration/native-chain` and `packages` matched the
fresh archive after LF normalization. The archived Lithic source SHA-256 was
`825951f69801a514d2f2763048b337fc0ac23c8129d34c6a74748bd5ef5ef56f`.
The accompanying Evmos lab archive at
`badff8be1a3bb565923ae63fd4f19599da4394d6` had SHA-256
`c18034075e2034ed4778efc6c4805aff08846d0463e9a39346d68ed688014f60`;
the SDK was the previously hash-verified
`f2e6295b662fdb27ea33da1296c29588ccdaab42` archive. A separate
smoke from these exact extracted archives built `lithovm-ffi` and `lithc`,
passed the disk-backed benchmark at 1 and 10 entries, and passed the tagged
Evmos store-upgrade test. The full 100k run itself was **not** repeated from
those fresh archives.

Raw result: [full Go test log](evidence/LITHIC_DURABLE_TOKEN_STATE_2026_10_01.log),
SHA-256 `12a7e8a8d5f9c74bb546bc650de5d8ca0fedf08bc23f7ae606221f89e9451dd0`.
The full test passed in 486.74 seconds. It populated holder-only,
allowance-only and mixed state at 1k, 10k and 100k seeded entries, committing
every 1,000 seeded operations and closing/reopening LevelDB after each size.
At each size it sampled reads, changed overwrites and unique-key growth 128
times, checking deterministic gas and committing every sampled write.

| State at 100k seed stage | Logical KV keys | Logical key+value bytes | LevelDB files |
| --- | ---: | ---: | ---: |
| Holders | 100,257 | 16,242,338 | 11,512,524 bytes |
| Allowances | 100,257 | 22,859,234 | 14,460,142 bytes |
| Mixed, 100k each | 200,513 | 39,100,706 | 25,972,865 bytes |

The extra 256 map keys per represented collection come from the earlier 1k
and 10k unique-growth sample stages; the contract record adds one key.
At 100k mixed state, the 90k-entry fill stage made 180,032 DB `Get` calls,
543,402 DB `Set`/batch-`Set` calls and 1,352 batch writes through the
instrumented Go DB boundary. These are logical method calls, not physical
NVMe read/write operations.

At 100k seed size, sampled execution p95 was 1.23–1.75 ms across the six
applicable read/overwrite/growth operations; the worst observed p99 was
3.81 ms for mixed allowance growth. Sampled write-commit p95 was
0.27–0.37 ms. The mixed-stage process reached roughly 475 MiB peak RSS.
Gas remained deterministic in each sample series: balance read 10,206,
overwrite 15,661, new-key growth 135,565; allowance read 10,605,
overwrite 17,842, new-key growth 177,746. These gas figures are the
**provisional disabled-candidate schedule**, not accepted production prices.

Reproduction from a pinned checkout on a Linux host with the FFI and compiler
built:

```sh
cd integration/native-chain
LITHOVM_DURABLE_BENCH=1 go test -mod=mod -tags lithovm_chain_lab \
  -run '^TestDurableTokenStateCandidate$' -count=1 -timeout 60m -v .
```

The benchmark is skipped unless `LITHOVM_DURABLE_BENCH=1` is set. Optional
`LITHOVM_BENCH_COUNTS` and `LITHOVM_BENCH_SAMPLES` only narrow or increase
the test workload; the recorded full run used 1k/10k/100k and 128 samples.

## Evidence boundary and remaining required runs

This is **not** a validator-class full-block result: no consensus scheduling,
ante path, EVM outer transaction, block commit, public RPC or indexer ran.
The close/reopen read does not drop the OS page cache, so it is not a true
cold-cache measurement. SDK `Commit` plus LevelDB reopen proves persisted
state across a clean restart, not crash durability or per-commit fsync. Linux
process I/O fields can remain zero while the kernel buffers writes; the
instrumented DB counts are logical calls. LevelDB file-size deltas can be
negative during compaction and must **not** be treated as logical state
deletion; the key/value totals are the stable state-growth measure.

The isolated deployment-size and log-envelope smoke cases are recorded in
[the boundary candidate](DURABLE_BOUNDARY_CANDIDATE_2026_10_01.md). They do
not close validator acceptance. Still required before Amir's production gas
approval: full-block maximum-state transitions and OOG/revert cases; warm and
true cold state; full blocks at multiple utilization levels through a
Foundation-approved finite test envelope; crash/restart evidence; and
p50/p95/p99 execution and durable commit time with CPU/RSS, DB operations and
logical growth for those blocks. The coordinated native-store upgrade/rollback
plan, independent security retest, final source pin and Makalu acceptance
remain separate gates.
