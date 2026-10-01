# Production Lithic and native LithoVM milestones

Status: approved combined workstream; implementation active

The production compiler and native runtime ship through one conformance gate.
Compiler output is accepted only when the native runtime independently decodes,
validates and executes the exact artifact with deterministic results.

## M1 — Versioned artifact and execution boundary

Status: implemented in candidate; verification metadata is now part of the artifact

- versioned, strictly decoded native artifact;
- fixed static value ABI and canonical encoding;
- deterministic function dispatch and gas rejection;
- compiler-to-runtime execution tests;
- fail-closed rejection of unsupported source and malformed bytecode.
- reproducible source/code hashes and canonical full-hash entrypoint selectors;
- portable artifact, verification-request and deployment-status schemas.

## M2 — Typed executable core

Status: typed expressions, locals, structured branches, bounded repeat loops, explicit failures and checked token amount operations implemented through version 11

- typed expression AST/IR (checked `u64` arithmetic and comparisons implemented in candidate);
- immutable local bindings and structured `if`/`else` implemented in the version 2 candidate;
- mutable local bindings and typed assignment implemented in the version 8 candidate;
- gas-bounded repeat loops implemented in the version 9 candidate;
- typed `require` and terminal `revert` implemented in version 10;
- checked `u256` addition, subtraction and greater-than-or-equal implemented in version 11;
- remaining general loops and bounded recursion policy;
- overflow, division, recursion and resource limits;
- compiler/runtime differential and fuzz tests.

## M3 — Transactional state

Status: scalar and typed map storage with explicit rollback implemented through version 11

- deterministic ordered scalar storage schema implemented;
- typed reads, writes and zero initialization implemented;
- staged atomic commit and rollback on runtime and out-of-gas failures implemented;
- explicit `require`/`revert` rollback across storage and staged host effects implemented;
- typed single and nested maps with deterministic multi-key encoding implemented in version 11;
- remaining general collections, migrations, namespacing integration and broader adversarial tests.

## M4 — Host effects and gas

Status: context, events, staged native transfers and outbound call intents implemented through version 7; transactional host/deployment candidate added

- caller, value, block and chain context implemented behind explicit runtime APIs;
- typed ordered event schemas and successful-call event records implemented;
- balance-checked staged native transfers implemented;
- balance-checked staged outbound call intents and a depth limit implemented;
- local [v13 synchronous calls](LITHOVM_SYNCHRONOUS_V13.md) now carry typed
  arguments/results, preserve effect ordering and reject reentrancy with full-tree
  rollback; local [v14 creation](LITHOVM_CREATION_V14.md) adds atomic child
  initialization and a finance-factory fixture. Persistent chain integration,
  creation receipts, salt/gas policy approval and security acceptance remain;
- context opcodes use the current versioned instruction gas schedule; host-call gas remains;
- deterministic receipts and observable failure semantics.
- atomic host deployment derives selectors from validated bytecode and commits
  code, value, initializer state and events together in the conformance adapter.

## M5 — LEP100-15

- domain-separated transaction digest and nonce policy;
- unique signer and threshold validation;
- atomic execution, signer changes and revocation;
- contract signatures, recovery and replay resistance;
- portable conformance vectors and negative tests.

## M6 — Makalu integration

Status: blocked on identifying and approving the authenticated Lithosphere
native runtime/RPC adapter; no Makalu deployment has been performed

- deterministic deployment and call interface in the L1 runtime;
- RPC, wallet, explorer and indexer support;
- real contract fixtures, upgrades, recovery and rollback rehearsal;
- performance and denial-of-service limits.

## M7 — Production release

- reproducible builds, SBOM and signed artifacts;
- compiler/runtime conformance, fuzzing and security review;
- versioning, compatibility and migration policy;
- `lithic.at` and `docs.lithic.at` updated to exact supported behavior;
- Makalu acceptance followed by explicit mainnet governance approval.

No milestone authorizes mainnet deployment by itself.
