# Isolated LithoVM/chain lab evidence (2026-09-26)

Status: local candidate evidence only. No LithoVM precompile was registered in
the Lithosphere app, no RPC exposed, and no Makalu/mainnet transaction or LAX
deployment occurred.

## Reproducible source boundary

The test Go module is `integration/native-chain`. Its local replacements point
to sibling *lab* worktrees, not the running chain: Evmos commit
`eca13ef2521a9ef13c32e80b1b147230bdb155b5` and Cosmos SDK v0.50.14 commit
`f2e6295b662fdb27ea33da1296c29588ccdaab42`. Both were assembled with the
six release-manifest patches in
`lithosphere-lithovm-integration/infra/litho-mainnet-9005/bin/patches`, after
checking their SHA-256 entries. The local pinned Go compiler is 1.22.12.
This evidence is not a claim that the app binary, build flags, runtime config
or currently running network match the lab.

## What was exercised

- Rust `Storage` has a bounded canonical persisted JSON format. Reload checks
  type/schema, duplicate/unknown fields and canonical encoding before use.
- `lithovm-ffi` exports a versioned C ABI with owned result buffer/free function,
  panic containment, bounded request/output, and a read-only host callback.
  It returns a validated write batch; C/Go callbacks do not commit state.
- Go applies successful writes to the real Evmos StateDB cache and adds a
  precompile journal entry. An enclosing snapshot revert removes native code,
  storage and logs. A later retry commits; a fresh StateDB reloads state.
- A native factory creates and initializes a child atomically. Failure leaves
  no child batch; outer revert removes successful child effects and logs.
- The `lithovm_chain_lab` build tag supplies an ephemeral `EVM.Call` precompile
  registry test. Its [canonical EVM ABI](NATIVE_GATEWAY_CANDIDATE_V1.md) now
  carries full native selectors and rejects trailing/aliased payloads. The
  request's caller and block fields are overwritten from the EVM frame/context.
  This registry is not installed in the chain app.
- Direct-wallet top-level deploy succeeds; an EVM wrapper's top-level deploy
  reverts before native execution. A wrapper may still call an existing native
  contract. Mismatched transaction-sender metadata is rejected.
- A test-only keeper seam now derives gateway sender/nonce from the Evmos
  `core.Message`, checks chain ID and EVM origin, and rejects fake simulation
  messages in commit mode. It does not yet run inside `ApplyMessageWithConfig`.

## Verification commands

From the toolchain root under WSL, after building the Rust shared library and
compiler:

```sh
cargo build -p lithovm-ffi -p lithc
cargo test --workspace --quiet
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cd integration/native-chain
CGO_ENABLED=1 go test -count=1 ./...
CGO_ENABLED=1 go test -tags lithovm_chain_lab -count=1 ./...
CGO_ENABLED=1 go test -race -tags lithovm_chain_lab -count=1 ./...
CGO_ENABLED=1 go test -run '^$' -bench BenchmarkGatewayDecode -benchtime=500ms -count=1 ./...
CGO_ENABLED=1 go test -run '^$' -fuzz FuzzGatewayABICanonical -fuzztime=20s -parallel=2 ./...
```

The gateway ABI fuzz campaign completed 397,163 executions without a crash or
noncanonical acceptance. On the local Linux/amd64 i7-8850H, decode measured
6,616 ns/op for a 324-byte input and 251,512 ns/op for a roughly 64 KiB input.
These are host-specific engineering measurements, not a consensus gas calibration
or a substitute for whole-frame/memory/FFI DoS benchmarks.

The Go tests require the pinned lab worktrees at the paths in `go.mod` and the
debug Rust shared library at `target/debug`. They are not a portable release
artifact or production installation recipe.

## Unclosed production controls

The internal FFI JSON request, live keeper registration, provisional ABI precharge
and dynamic copy pricing, event topic and native state namespace are not
consensus-approved. Malformed-payload decoding now has an experimental static
precharge; complete gas accounting and benchmarks remain open.
Native value, bank/EVM balances, native-to-EVM callbacks and general arrays are
unsupported. The lab has no live transaction authentication, signed broadcast,
RPC, indexed/finalized receipt, network upgrade, replay test, independent
security review or Makalu recovery test. The client confirmed gateway route,
salted child identity, fail-whole policy and direct-wallet deployment; consensus gas/ABI, salt
reservation/collision, receipt and upgrade reviews remain required before
production-facing registration or activation.
