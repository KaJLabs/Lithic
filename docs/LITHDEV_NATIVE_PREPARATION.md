# Native gateway preparation with lithdev

`lithdev` compiles actual source and prepares canonical unsigned native gateway
calldata. It also decodes gateway return bytes into checked native values.
It is included with the combined compiler/runtime release and is available from
the SDK's `tooling/bin/lithdev` wrapper during development.

The supported transport profile is Makalu EVM chain 700777, gateway
`0x000000000000000000000000000000000000f000`, with
`deploy(bytes,bytes32,bytes)` and `call(address,bytes32,bytes)`.
This is the October native gateway profile, also described by the separate
`NATIVE_LITHIC_DEVELOPER_HANDOFF_2026-10-26.zip`. It does not establish a mainnet
profile or authorize any transaction. Native target identities are distinct from
ERC-20 contract addresses; use the gateway route rather than an ERC-20 adapter.

## Prepare deployment and calls

From the repository root, using a packaged `lithdev` executable:

```sh
lithdev prepare-deploy apps/examples/frontend/lithovm-storage.lithic --chain-id 700777
lithdev prepare-call apps/examples/frontend/lithovm-storage.lithic \
  --chain-id 700777 --contract 0x1111111111111111111111111111111111111111 --function get
```

The address above is an illustrative target, not a claimed deployment. The
storage example is a codec/development fixture and does not restrict writes.
Use the actual independently verified target and matching source for a real
integration.

To prepare `set(u64)`, provide a JSON file containing:

```json
[{"type":"u64","value":"42"}]
```

```sh
lithdev prepare-call apps/examples/frontend/lithovm-storage.lithic \
  --chain-id 700777 --contract 0x1111111111111111111111111111111111111111 \
  --function set --args args.json
```

For a source that declares `initialize`, deployment requires explicit
`--initializer initialize`, plus the corresponding typed `--args` file. Code,
full 32-byte initializer selector and arguments are encoded together for atomic
deployment/initialization. A contract with no initializer uses a zero selector
and empty gateway argument bytes. Contracts may have another explicitly chosen
initializer, but a declared `initialize` cannot be bypassed by selecting a
different function.

Output contains the compiled artifact, source/code hashes, exact calldata,
native function signature, gateway, chain and zero transaction value. It records
`executionAuthorized: false` and `deploymentStatus: "not-submitted"`. Sender,
nonce, gas and fee fields are intentionally left pending for the established
wallet/execution workflow. No deployment address or nonce reservation is inferred.
Compiling matching local source does not prove what code is deployed at a target.

## Argument and result validation

- `u64` and `u256` require canonical decimal strings. JSON numbers, negative
  values, fractions, leading zeros and overflow are rejected.
- `bool` requires JSON `true` or `false`.
- `address` requires 20 hex bytes; `bytes32` requires 32. The `0x` prefix is required.
- `string` preserves UTF-8, with a 4,096-byte per-string limit.
- Argument types/count must match the compiled signature exactly. Duplicate
  JSON fields and unknown argument properties are rejected.
- Native envelopes have at most 64 values and 65,536 encoded bytes. Source and
  argument files are bounded to 4 MiB before parsing.

`lithdev decode-result 0x...` accepts the canonical EVM `bytes` return envelope
and decodes LVAL v1. It rejects noncanonical offsets, padding, scalar words,
invalid UTF-8, bounds violations and trailing bytes. Integer output stays in
decimal strings without JavaScript-number precision loss. Decoded results are
observations, with `canonicalProofVerified: false`.

## Execution boundary

There is no RPC, key handling, wallet connection, signing, estimate or broadcast
command in lithdev. The existing execution workflow must verify actual chain,
runtime, account/nonce and deployed code; simulate; bind transaction fees/limits;
verify returned signed bytes; and apply its established authority/finality rules.
Mainnet chain 9005 is rejected by this profile.

The code-size admission boundary is 64 KiB; admission does not guarantee that
deployment fits the active native gas envelope. Native-envelope exhaustion can
burn the remaining gas of the affected EVM frame under the accepted LTH-10
behavior. Do not forward excessive gas or treat a prepared intent as an approved
gas budget. This CLI does not change native gas policy or override any execution
window, one-shot limit or stop condition.

## Verification

Rust tests cover public CLI execution, decimal boundaries/precision, typed
arguments, UTF-8 byte bounds, duplicate/unknown fields, atomic initialization,
strict return decoding and option/profile failures. The gateway/LVAL bytes also
match three frozen vectors generated independently using the supplied JavaScript
reference and ethers 6.15.0. Those transport vectors include opaque code bytes
for ABI testing; they are not deployable contracts. The release smoke exercises
the packaged lithdev alongside the packaged compiler, verifier and FFI library.
