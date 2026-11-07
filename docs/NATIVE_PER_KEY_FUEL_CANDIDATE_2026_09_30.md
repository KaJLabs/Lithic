# Per-key native state and live fuel candidate — 2026-09-30

Status: **disabled lab candidate only**. This work does not approve gateway
registration, Makalu deployment, production pricing, LAX custody or activation.
The implementation is pinned in local commits: Lithic
`2d755aeed8b7fc4826b92a139b73030a02693c81` and Evmos lab
`24e674de1bd4150025562aa6e393a672736feb65`, against SDK lab
`f2e6295b662fdb27ea33da1296c29588ccdaab42`. These commits are not a
published or independently retested release artifact.

## State layout and rollback

- The existing `lithovm/v1/contracts/<20-byte-address-hex>` key now holds a
  version-2 record with code hash, code and **scalar-only** storage. The key
  name is retained to keep the code lookup stable, but the record version is
  consensus-significant.
- Each map word is stored at
  `lithovm/v2/maps/<address>/<field>/<concatenated-32-byte-keys-hex>` and has
  exactly 32 value bytes. The FFI and Go bridge both validate canonical key
  shape. The VM lazily resolves an absent map entry only when an instruction
  reads it; writes stage only touched entries. All staged writes are emitted
  as one batch and applied only after validation and charging. Nested native
  calls see staged map writes through the transaction-scoped map overlay.
- Identifier and nested-key limits match validated bytecode (255 name bytes,
  up to 64 typed 32-byte keys). The callback caps encoded key bytes at 8192.
- Version-1 inline-map records are rejected; no implicit migration occurs on
  a getter or transfer. A separately reviewed migration would be required if
  any such state ever existed on-chain. There is no registered live native
  state to migrate today.
- A zero-valued map write emits a map-key deletion in the same atomic batch.
  Go rejects deletion of contract metadata and rejects non-deletion map
  values that are not exactly 32 bytes. There is no gas refund; deletion pays
  the provisional changed-write rate with no additional growth charge.
  The disabled Evmos lab now mounts a separate native KV store and has
  versioned genesis export/import checks. The store-upgrade path on an
  existing chain and production-scale genesis benchmarks are **not finished**.

## Live fuel boundary

The new FFI v2 entrypoint takes a charge callback. Each VM charge, including
initial call fuel and child-creation fuel, debits the same Go `frameMeter` that
charges physical KV reads, changed writes, state growth and emitted logs.
Synchronous child gas is added to the parent's local total without a second
external debit. A failed debit exhausts the EVM frame and discards the staged
batch. The existing FFI v1 entrypoint remains for isolated unmetered
conformance use; `ExecuteFrame` uses v2. The 10M aggregate envelope and
growth rate remain provisional, not production economics.

## Local conformance evidence

On WSL/Linux, `cargo test --workspace --locked --quiet` passed after the
storage/fuel changes; focused Rust tests cover scoped fuel, record integrity
and refusal of legacy inline records. The isolated VPS working copy passed
the tagged native-chain Go suite, including exact combined KV/VM fuel
boundaries, a 1,000-holder per-key state check, flat gas across new holders,
and an OOG write with no persisted holder. A separate adversarial test
corrupts a map value, verifies failure without writes, restores it and
verifies read/overwrite recovery; it then clears the word and verifies the
persistent key is removed and future reads return zero. The tagged full
Evmos keeper suite and targeted keeper race test also passed against that
  working copy after the deletion change. A separate cross-frame test writes
  a map word in one synchronous child call, reads it in a subsequent child
  call in the same parent transaction, and confirms a parent revert discards
  it. The native genesis codec and tagged/ordinary Evmos app/keeper suites
  also pass; see [native-store lifecycle evidence](NATIVE_STORE_LAB_LIFECYCLE_2026_09_30.md).
  `cargo fmt --check`, workspace
clippy with `-D warnings`, and `gofmt -l` also passed. These
are in-memory/isolated correctness checks, **not** durable full-block
benchmarks or evidence of 10k/100k-holder affordability.

## Pinned archive rerun

On the isolated VPS, fresh `git archive --format=tar.gz` exports were staged
under `/var/lib/lithic-bench/source/pinned-20260930-v1`. SHA-256 was checked
on the VPS before extraction:

| Source pin | Archive SHA-256 |
| --- | --- |
| Lithic `7b91097fdc0d323335b8d0b0b30fb465fcdee5a6` (documentation descendant of implementation pin) | `4655b8c6b8ddd4d75869749ac765fc8f12e298933f9540ea82369742e22b768f` |
| Evmos lab `24e674de1bd4150025562aa6e393a672736feb65` | `edb6ba11db884f93faa7e23c7413d5dc3e8b8bfbb4b61a9bad093ed6ab39844e` |
| SDK lab `f2e6295b662fdb27ea33da1296c29588ccdaab42` | `f21aef4886bee1f10d71052ac144d0a270e6c28a0a135cfccee1cf222d3aa796` |

From those extracted sources, `cargo build --locked -p lithovm-ffi -p lithc`
passed. `go test -mod=mod -tags lithovm_chain_lab -count=1 .` passed in
`integration/native-chain`; tagged and ordinary
`go test -mod=mod -count=1 ./x/evm/keeper ./app` passed in the Evmos lab
(tagged command additionally used `-tags lithovm_chain_lab`). The first Go
attempt on the fresh archive failed only because the Rust FFI and compiler
binaries had not yet been built; both were built from the pinned Lithic source
before the passing rerun. This is a source-pin conformance check, not an
independent audit or durable validator benchmark.

The [disk-backed 1k/10k/100k token-state benchmark](DURABLE_TOKEN_STATE_BENCH_2026_10_01.md)
now provides isolated GoLevelDB/IAVL evidence; it does not satisfy full-block
validator acceptance. Before a release candidate: design and test a
chain-approved store upgrade; run the remaining full-block and worst-case
validator workloads;
obtain Foundation approval of the final pricing envelope (Alex has accepted
the 350 GiB isolated benchmark host); then seek independent security retesting and Makalu
end-to-end approval. No production or LAX deployment is authorized.
