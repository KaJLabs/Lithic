# Lithic implementation status

This repository contains a preview compiler backend plus development scaffolds. Its design documents describe intended capabilities; they do not establish production availability.

| Tool | Current implementation |
|---|---|
| lithc | Parses/checks declarations and emits ABI plus executable EVM or versioned native LithoVM bytecode for the documented subset. Native v11 artifacts include compiler/target versions, exact source/code hashes and canonical full-hash entrypoint selectors. Unsupported semantics fail the complete build. |
| lithovm-host | Transactional chain-state seam plus in-memory conformance adapter. Produces structured failure/gas outcomes, atomically executes call trees, and rejects reentrancy. Deployment v1 derives selectors from validated bytecode and adds code hashes, deterministic addresses, value transfer and optional initializer execution in one transaction. No Lithosphere persistence/RPC adapter is included. |
| lithfmt | Parse-checked, literal-preserving whitespace normalization; supports --check. |
| lithlint | Declaration-level naming and AI-budget rules; supports --deny-warnings. Not a security analyzer. |
| lithdev | Placeholder shell entrypoint. No deployment execution. |
| lithls, lithtest, lithsec, lithpkg | Specification-only targets. No usable implementations here. |

The SDK compiler, formatter and linter wrappers invoke the Rust commands from this checkout. lithdev remains a placeholder. The native VM strictly decodes and executes [Native LithoVM ABI v11](LITHOVM_ABI_V11.md) and versions 1 through 10. Version 11 adds typed map storage and checked `u256` add/subtract/comparison. Version 10 adds explicit transactional `require` and `revert`. Versions 9 through 3 add bounded loops, mutable locals, staged calls, transfers, events, context and scalar storage. The [transactional host v1](LITHOVM_HOST_V1.md) retains deferred zero-argument legacy calls. Local v12/v13 candidates described below add dynamic values and synchronous typed calls. General collections, persisted consensus receipts and zk authorization remain unimplemented. The EVM target remains documented in [EVM backend v1](EVM_BACKEND_V1.md).

## Local development

The local [v12 string candidate](LITHOVM_DYNAMIC_VALUES_V1.md) additionally
compiles and executes string parameters, locals, scalar storage, events,
equality and returns through the explicit dynamic-value VM API. It includes
byte limits, byte-dependent gas and rollback tests. Explicit dynamic host APIs
support atomic initialization, typed top-level calls/results and transaction-wide
event limits; scalar APIs still reject dynamic bytecode. Source string literals,
string maps and native chain integration remain unfinished. This candidate is
not included in preview.3 and is not a production release.

The local [v13 synchronous-call candidate](LITHOVM_SYNCHRONOUS_V13.md) adds
`let result: Type = invoke(target, selector, value, args...);` across the compiler,
VM and typed transactional host. Caller execution resumes with typed return data;
events/transfers follow source order and any failure discards the whole transaction.
Legacy frames retain deferred semantics. This does not provide native-to-EVM
calls or a deployed native chain interface.

The local [v14 creation candidate](LITHOVM_CREATION_V14.md) adds atomic child
creation from deployed code templates, mandatory initialization, deterministic
salted addresses and rollback. A finance factory fixture initializes fresh v12
tokens across all 16 feature profiles with creator-owned supply. Salt policy,
consensus pricing, persisted creation receipts, security review and native chain
integration remain open; this is not a production factory release.

An [isolated native-chain lab](NATIVE_CHAIN_LAB_EVIDENCE_2026_09_26.md) now tests
bounded persisted storage, a Rust C ABI, Go StateDB rollback/commit/reload and
an ephemeral EVM precompile call against pinned Evmos/SDK sources. This is not
an app registration, deployed RPC, Makalu contract or production gas/ABI rule.
The client-confirmed first profile has a [strict EVM ABI candidate](NATIVE_GATEWAY_CANDIDATE_V1.md)
with full native selectors, provisional gas precharge and direct-wallet-only
top-level deployment; it remains test-only.
The [disabled keeper lab](NATIVE_CHAIN_KEEPER_LAB_2026_09_27.md) additionally
passes a signed local EVM deploy/call and recovery through pinned Evmos
`ApplyMessageWithConfig`. It is not a Makalu transaction or release build.

Install Rust and the platform C/C++ linker, then run from this repository:

```sh
cargo build --workspace
cargo test --workspace
cargo run -p lithc -- --help
```

These commands build and test the toolchain. Passing them validates only the capability matrix above. No production installation, signing or deployment command is included.

Reviewed tags can produce draft cross-platform archives and checksums through
the [preview release process](RELEASE_PROCESS.md).

The independent `fuzz` workspace contains libFuzzer targets for strict
bytecode decoding and the compiler-to-runtime artifact seam. CI runs bounded
smoke campaigns; see the dated fuzz evidence for the latest local run. These
campaigns do not replace a production security review.

## Lithosphere integration

A [local execution experiment](LITHOVM_EXECUTION_LAB.md) remains historical.
`lithc --emit lithovm` now emits the versioned native artifact described in
[Native LithoVM ABI v11](LITHOVM_ABI_V11.md) and the
[verification interface](LITHOVM_VERIFICATION_V1.md), while retaining v1 through v10 decoding compatibility. This establishes the combined
compiler/runtime boundary; it does not yet establish production readiness.

The tested front end from `KaJLabs/Lithosphere/toolchain` has been imported here. See [import provenance and command changes](FRONTEND_IMPORT.md).

The EVM backend targets the deployed EVM interface on LITHO. The native
compiler, interpreter and transactional host are currently library-level
components and do not
claim that a `lithic_*` RPC namespace or on-chain native module is deployed.

## LEP100-15

LEP100-15 is a draft multisignature smart-account standard. See [the supplied draft](../packages/standards/lep100/LEP100-15.md). Its example contracts, SDK calls and deployment configurations are illustrative. No implementation or mainnet address is certified by inclusion in these docs.

Interoperable signing also requires the unresolved normative values listed in
[LEP100-15 implementation decisions](LEP100_15_IMPLEMENTATION_DECISIONS.md).

Core acceptance requires deterministic signing vectors, domain and nonce replay protection, threshold and unique-signer checks, revocation, atomic execution, reentrancy protection, authorized signer changes, custody tests and contract-signature tests against the supported runtime. Recovery and optional AI/agent profiles require their own tests. Partial implementation must identify unsupported features.
