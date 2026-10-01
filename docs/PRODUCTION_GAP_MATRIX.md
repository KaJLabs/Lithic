# Lithic/LithoVM preview.3 to production gap matrix

Verified: 2026-09-26

Baseline: annotated tag `lithic-v0.2.0-preview.3` at
`b5547d408d7f461d285402f6329090940d54eeae`. The tag itself has no Git
signature. GitHub Actions run `35652881357` succeeded and its Linux archive
has valid GitHub/Sigstore build provenance bound to that commit. All three
published archive hashes match their adjacent SHA-256 files:

| Platform | SHA-256 |
| --- | --- |
| Linux x86_64 | `3eda35ff5bab5da445d76e00272e591792986539305920d39b00df8bb57b4ed2` |
| Windows x86_64 | `54f63577a6aa9fddb58bf891e39a9ef07df171668472a3ce524fccbae3577fc6` |
| macOS arm64 | `a20695801b9fe964943a725b1f93d64d5aa8929d6ebb93f586a9ecff0b5be2fe` |

At verification time KaJLabs/Lithic had no open pull requests and no open
issues. The preview.3 source passed all 82 workspace tests on Linux. Windows
source tests could not link locally because MSVC Build Tools are absent.

Status meanings: **implemented** means exercised through the public
compiler/runtime seam; **partial** means useful implementation exists but the
production requirement is not closed; **missing** means no executable path was
found. Documentation and pseudocode are not counted as implementation.

| Production capability | preview.3 actual state | Current candidate | Exact production gap / acceptance evidence |
| --- | --- | --- | --- |
| Typed expressions and control flow | Partial: `u64` checked arithmetic/comparison/equality, typed constants/locals, `if/else`, bounded `repeat`; versions 2, 8 and 9 | v10 adds failures; v11 adds checked `u256` add/subtract and `>=` needed by token balances | Add remaining approved operations, boolean operators, function calls, approved recursion policy or explicit rejection; differential/property fuzzing and limits evidence |
| State and storage | Partial: deterministic scalar fields, zero initialization, staged clone-and-commit | v11 typed maps; bounded canonical Rust storage snapshot and real SDK KV reload tested in isolated chain lab | Production keeper/namespace, layout/version migration rules, general collections and adversarial persistence/security review |
| Events | Partial: typed schema and ordered staged records | v12 bounded dynamic fields; v13 source-order cross-frame journal and aggregate event-cap rollback/recovery tests | Consensus event commit, RPC/indexer mapping, topic/indexing rules, Makalu observation tests |
| Contract calls | Partial: typed target/selector/value intents, balance/depth checks | v13 typed synchronous calls; v14 atomic template-based creation and initializer execution; legacy frames retain deferred semantics | Approved native-to-EVM policy, persistent Lithosphere adapter, depth/DoS review and nested Makalu vectors |
| Native transfers | Partial: balance-checked staged intents | Host conformance adapter atomically debits contracts, credits native accounts and rolls back transfers on nested failure | Consensus bank/EVM adapter, address/denom rules and Makalu success/failure tests |
| Rollback and recovery | Partial: storage clones roll back runtime and out-of-gas errors; effects exist only in successful results | Structured failures plus isolated pinned-source Evmos StateDB tests: parent revert removes native state/logs; retry persists and reloads | Full transaction/restart recovery against the approved Lithosphere app and Makalu rehearsal |
| ABI and deployment | Partial: deterministic native artifact/call ABI; CLI emits JSON/hex | Canonical selectors/artifact hashes and atomic top-level deployment; v14 salted child creation; strict EVM gateway ABI tests direct-wallet-only deploy; test-only Evmos message seam passes nonce/chain ID and simulation guards | Reviewed salt reservation/collision, live keeper registration and replay, consensus ABI/gas, persisted receipts, RPC, upgrades, signing/broadcast and lithdev implementation |
| Gas semantics | Partial: deterministic instruction/statement schedule, host-intent charges and OOG rejection | VM failures report consumed gas; test-only gateway prepays bounded ABI decode and charges native/read/write/log work | Consensus-approved schedule, memory/event/calldata pricing, failure/refund decision, chain integration and DoS benchmarks |
| LEP100 token | Missing executable implementation in preview.3 | v11 compiles and executes the LAX candidate with exact initial supply, metadata getters, balances, nested allowances, transfers, approvals, delegated transfers, burn, events and negative rollback vectors | Approve the executable profile, atomic deploy/init ABI, zero-address and allowance policy, distribution input, more portable conformance vectors, chain integration and audit |
| LEP100-15 | Missing: the standard is Draft and no account implementation or vectors exist | Failure primitives are groundwork only | Resolve normative signing/domain values; implement nonce/replay, unique signers/threshold, revocation, atomic batch, self-authorized changes, contract signatures, custody/recovery, reentrancy and portable vectors |
| Makalu | Missing native chain integration: library runtime only; no native RPC/module identified | Isolated pinned Evmos/SDK keeper lab has a build-tagged gateway hook; signed local EVM deploy/call, simulation discard, failure/recovery and default-disabled tests pass; no network transaction or release build | Reviewed reproducible L1 patch/manifest, consensus ABI/gas/identity, authenticated Makalu broadcast/RPC, real contracts and failure/recovery/upgrade/DoS receipts |
| Fuzzing/security | Missing production evidence: bounded unit tests exist; no fuzz target, corpus or current toolchain security review | Decoder/compiler-runtime, stateful map, synchronous call and child-creation targets; bounded execution campaigns and generated corpora retained | Longer campaigns, broader graphs/differentials, coverage/dependency/SBOM review, threat model, independent review and remediation closure |
| Reproducible signed release | Partial: locked Rust build, pinned Actions, checksums and build provenance; archives are produced independently per hosted runner | Unchanged | Rebuild comparison or documented reproducibility level, SBOMs, malware/dependency scan, signed annotated release tag or approved keyless tag policy, installation/conformance tests |
| litho.finance deployment UI | Missing from this repository; no production deployment interface exists to integrate | Full frontend accessed; local safety branch through `a38191c` rejects fake native success and validates Solidity receipts; 24 unit tests, typecheck, lint, production build and four isolated browser tests pass; native remains disabled | Real wallet/signing tests, reviewed native ABI, simulation/fee/network checks, status/retry UX and end-to-end Makalu evidence; no live change performed |
| LithoScan verification/status | Missing native source-verification pipeline | Offline lithverify rebuilds exact source and checks full artifact/code metadata; host now returns committed deployment records with a tested branch-bound included/unverified status mapper | Approved receipt transport, trusted native RPC and pinned-height code, compiler provenance/worker limits, persisted status/finality and UI |
| LAX reference token | No production Lithic contract or deployment authorization | Source candidate compiles under v11, initializes exactly `10_000_000_000 * 10^18`, and passes atomic host deploy/init; no deployment performed | Approved LITHO allocation input, chain preservation of deploy/init atomicity, executable-profile approval, audit, Makalu rehearsal, verification package and explicit deployment approval |

## Ordered closure path

### Current external decision gate

Host-generated deployment records and the local included-status handoff are
implemented; see [receipt evidence and remaining integration](LITHOSCAN_NATIVE_RECEIPTS_V1.md).
The client confirmed EVM gateway, salted child identity, fail-whole native
transaction semantics and direct-wallet-only top-level deployment for the first
profile on 2026-09-26. The resulting
[strict gateway ABI candidate](NATIVE_GATEWAY_CANDIDATE_V1.md) is still local.
Consensus gas/ABI, salt reservation, receipt and upgrade review must precede
app registration, RPC or Makalu activation.
LEP100-15 also still needs the previously identified normative signing values.
This does not imply other engineering, Makalu tests, audit or releases are complete.

### Local string milestone, 2026-09-26

The current candidate now supports compiler-to-VM string parameters, locals,
scalar storage, events, equality and returns under bytecode v12, with bounded
UTF-8 values, byte-dependent gas and transactional rollback tests. Scalar
programs still emit v11. See [exact supported behavior and
limits](LITHOVM_DYNAMIC_VALUES_V1.md). This advances the language, storage,
events and ABI rows; it does not close any production row. The host now has
explicit dynamic-value deploy/call APIs alongside the scalar APIs. Dynamic
initialization is atomic; nested failures roll back strings and value transfers;
transaction-wide event limits are enforced. A separate v12 finance-token fixture
adds name/symbol and passes atomic initialization/getter tests for all 16 feature
profiles. Source literals and consensus integration remain necessary; local
factory creation is recorded below.

### Local synchronous-call milestone, 2026-09-26

The [v13 candidate](LITHOVM_SYNCHRONOUS_V13.md) now implements ordered native
calls with typed arguments and return data across compiler, VM and host. Eight
new host tests cover ordering, nested returns, exact gas, rollback/recovery,
invalid calls and failed initialization. The `synchronous_calls` fuzz target
exercises executable call-tree invariants. This advances the call/effect rows;
approved factory policy, EVM interoperability, persistent chain execution and production
security acceptance remain open. No production row is closed by local tests.

### Local child-creation milestone, 2026-09-26

The [v14 candidate](LITHOVM_CREATION_V14.md) adds deterministic template-based
child creation, mandatory initialization, byte-dependent creation gas and full
transaction rollback. A finance factory fixture initializes fresh v12 tokens for
all 16 profiles, preserving user metadata/supply/ownership and template state.
Five new host tests and a 35,960-execution fuzz smoke pass. Salt/front-running
policy, consensus pricing/receipts, persisted execution, security review and
Makalu acceptance remain open. See [dated evidence](CREATION_CANDIDATE_EVIDENCE_2026_09_26.md).

### Closure sequence

1. Merge and review v10 explicit failure semantics.
2. Define the persistent host interface, structured failure/gas receipt and
   nested call atomicity; implement it with an in-memory conformance adapter.
3. Add collection storage and the integer/authorization primitives required by
   an executable LEP100 token, then build conformance vectors.
4. Implement the LEP100-15 core only after its unresolved signing decisions
   are approved.
5. Integrate the native host into Lithosphere and validate deployments,
   calls, rollback, recovery and limits on Makalu.
6. Integrate the approved deploy/status interfaces with litho.finance and
   LithoScan.
7. Complete fuzzing, security review, reproducibility/SBOM evidence and
   documentation; request production and LAX deployment approvals.

No row in this matrix authorizes a production or LAX deployment.
