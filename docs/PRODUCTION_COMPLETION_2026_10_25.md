# Compiler/runtime completion record — 25 October 2026

Scope: standalone Lithic compiler, native runtime and release tooling. LAX token
execution/distribution is being completed separately. Historical audit packages,
approved node binaries and deployed services are not replaced by this work.

Update 5 November: the former lithdev placeholder is replaced with native
deployment/call intent preparation and strict gateway-result decoding. It is
included in the reproducible release and packaged smoke. See
[native tooling usage](LITHDEV_NATIVE_PREPARATION.md). It does not perform the
wallet/RPC execution steps or close the standards/performance gates below.

## Completed local release milestone

`scripts/release_toolchain.py` builds `lithc`, `lithverify`, `lithdev`, `lithfmt`,
`lithlint` and `lithovm-ffi` from the same locked snapshot. Two independent build
roots must produce identical binaries. Deterministic archives carry the C header, source
inventory, lockfile, SBOM, binary checksums and build/smoke evidence. Packaged
compiler-to-FFI execution and source-verifier tamper checks pass on native Linux
x86_64, Windows x86_64/MSVC and macOS Apple Silicon. Rust workspace tests, fmt and
clippy pass on all three platforms. The cross-platform release workflow runs
these checks before producing attested draft release assets.
The README no longer labels the implementation as final.

The initial local exercise used a recorded dirty checkout. A separate clean
release branch binds the release-only changes to a commit; each archive records
its actual source pin, source state and input hashes. On 5 November, all six
native-platform jobs passed for release branch head
`1d15ca792efcff7459452029ad0dc3f0886f7a90`:

- [Native rebuild, archive and packaged execution checks](https://github.com/bachal-mb/Lithic/actions/runs/41907917120).
- [Rust fmt, clippy and workspace tests](https://github.com/bachal-mb/Lithic/actions/runs/41907917660).

The archives identify CI merge commit
`2a48dc414ce9b6e8a4dd77ebf6b9af3139b48b50`, whose parents are that release head
and unchanged base `f7c259ae0a8e5d9e5fb6e7b0b7cbac1428fac8ce`.
Their downloaded inventories, SHA-256 checksums and embedded smoke records
verify; compiled smoke source/code hashes agree across platforms. Each platform
compares two independent native build roots, not binaries across operating
systems. Local release/codec regressions pass 19 checks; Rust tests total 164.

The first Windows/macOS comparisons failed and remain recorded as failures.
Their retained binaries exposed MSVC timestamps/CodeView IDs and a temporary
macOS dylib install name. Link-time `/Brepro` and a stable `@rpath` install name
resolved those differences without post-build rewriting or relaxed comparison.

| Native archive | SHA-256 |
| --- | --- |
| Linux x86_64 | `f679066f99345b6d35ae5640aaee71e1a79202aee97337992c6344fa929b5840` |
| Windows x86_64/MSVC | `1c65e7c7a53c6bed6d90e4e8c46a9742084d7114d6721ddc67b08f02324e1a98` |
| macOS Apple Silicon | `7463aa2894e091338dd21ed7760cac78c5cd70e88f516ff05a01da21c0ca95cd` |

These are unsigned CI candidates, version
`lithic-v0.2.0-production-candidate-ci`, retained for review. They are not a
published production release or third-party reproduction. The changes are in
[draft fork PR #1](https://github.com/bachal-mb/Lithic/pull/1), stacked on the
existing `feat/deployment-interface` branch used by upstream PR #13. Neither
upstream merge nor production release acceptance is inferred. The evidence
above applies to the recorded source pin; subsequent documentation edits do not
change those historical archive hashes.

## Current capability gaps from preview.3

Update 6 November, separate development branch: bounded binary `bytes` and
runtime Keccak-256 now have a compiler-to-host/FFI implementation under
[native bytecode v15](LITHOVM_DYNAMIC_VALUES_V1.md#binary-values-and-runtime-hashing-candidate-2026-11-06).
The reviewed release and benchmark candidate remain unchanged. These primitives
are groundwork for native smart accounts; they do not establish LEP100-15
conformance. The canonical signing profile/vectors have been submitted in
[official PR #14](https://github.com/KaJLabs/Lithic/pull/14), with no maintainer
decision recorded as of this update.

The [September baseline matrix](PRODUCTION_GAP_MATRIX.md) records preview.3 and
the then-local candidate. Use the following current boundaries with the
remaining production gates below; September network-availability statements are
historical.

| Capability | Verified implementation available for review | Remaining required closure |
| --- | --- | --- |
| Typed expressions/control flow | Typed scalar values, checked integer operations, locals, branches and bounded loops through compiler/runtime tests | Accept the supported language profile; unsupported operations, general arrays and syntax must retain explicit rejection |
| State/storage | Scalar state, typed/nested maps, dynamic strings, transactional host and FFI per-key persistence | Match the final chain adapter/store lifecycle and scalability evidence to the selected source and approved workload matrix |
| Events, calls and native transfers | Typed events, v13 synchronous native calls/returns, v14 child creation, value transfer and transaction-wide rollback tests | Final chain-specific ordering, receipt/indexer and client evidence; native-to-EVM ERC-20 interoperability remains unsupported |
| Failure/recovery and gas | OOG/revert rollback, shared live FFI fuel accounting, bounded effects and recovery tests | Final workload/state-size evidence and accepted cap/growth/economics; preserve accepted LTH-10 frame-burn behavior in integrator guidance |
| ABI/deployment tooling | Atomic deployment/initialization, caller-bound child salts, strict selectors/typed envelopes, packaged lithdev intent preparation and decoding | Wallet/RPC execution, authenticated code/runtime checks and final integration acceptance against the chain build |
| LEP100 reference profile | Executable balances/allowances/transfer/burn fixtures and rollback tests | Final reference-token and allocation/deployment work belongs to the separate LAX session |
| LEP100-15 | Proposed canonical signing profile/vectors in official PR #14; separate local binary/hash primitives | Maintainer decision, remaining native signing/account primitives and native smart-account implementation/conformance; no conformance claim yet |
| Makalu | Separate 23 October activation evidence, described below | Final selected-source compatibility and required authenticated end-to-end failure/recovery evidence; no mainnet acceptance inferred |
| litho.finance and LithoScan | Gateway preparation/codec vectors, source rebuild verifier, host deployment journal and included-status mapper | Final installed frontend/indexer/verification and trusted status/finality acceptance |
| Fuzzing/security/release | Existing fuzz targets/campaign evidence, dependency scope check, native-platform CI and identical rebuilds | Final-source independent security review, approved signing/provenance and publication/rollout decision |

The language/tooling/runtime implementations are reviewable engineering results.
They do not turn an unresolved required capability or failed benchmark into a
production pass.

## Dependency evidence

The 5 November OSV lookup covered 257 locked crates.io packages at lockfile
SHA-256 `be8da7df769198e7670054a37554d1e244597909d6e202bf99702818adb18a15`.
Three records remain disclosed: `crypto-bigint 0.5.5` timing advisory
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html), and
unmaintained `derivative 2.2.0` / `paste 1.0.15`. Inverse Cargo normal/build
graphs across all targets have no path to those packages under the selected
features. `crypto-bigint` and `paste` are reachable through development/test
dependencies; `derivative` is an unselected optional lock entry. No dependency
was removed or finding suppressed to obtain this scoped result. It does not
clear those advisories or establish security acceptance.

## Actual remaining production gates

| Area | Required closure | Current boundary |
| --- | --- | --- |
| Language/runtime support | Freeze the supported production language/ABI profile and reject all unsupported semantics; resolve required collection/operation gaps against real integration cases | Typed maps, strings and synchronous calls/creation are implemented; general arrays and other documented unsupported forms must not be advertised as implemented |
| LEP100-15 | Obtain the maintainers' hash-bound profile/vector decision, implement/review native account semantics and pass positive/negative replay/threshold/atomicity tests | PR #14 remains proposed. Binary/hash primitives do not implement signature verification or an account. An EVM vault's own suite does not establish native conformance |
| Validator performance/economics | Complete the authorized sustained workload/state-size matrix, derive resource/state-growth budgets and approve cap/gas policy | The accepted 25M slice is limited evidence. The full 50M benchmark remains failed. R6.4 public evidence checks passed for 2,201 zero-failure workload transactions within the approved bounds; owner-private stacks were delivered for independent pinned-checker replay/final closeout. No new workload is authorized |
| Chain compatibility/integration | Match accepted compiler/runtime/library pins to the chain build and complete authenticated deployment/calls, rollback/OOG/recovery, verification/status and client end-to-end evidence | Makalu native activation has separate evidence; token-specific rehearsal belongs to the LAX session. Mainnet activation is not established by local release tooling |
| Security and release | Review current dependency/SBOM and fuzz/coverage evidence, close findings against the final source, verify approved signing/provenance and authorize publication/rollout | Native-platform rebuild/package and Rust checks pass at the source pin above; unsigned CI candidates and dependency scope evidence are available. Independent reproduction, signing and final security acceptance remain open |

## Chain evidence correction

Older milestone/gap documents describe the pre-integration September snapshot.
The surrounding workspace's
`client-work/LAX_MAKALU_NATIVE_ACTIVATION_RESULT_2026-10-23.zip` records activation
of the accepted native application on `lithosphere_700777-2` at height 18,265,000:
three-node code/config checks, matching signed H/H+1 headers and native-store
queries. It explicitly leaves installed integration and conformance items open.
This is dated chain evidence, not a fresh RPC assertion or production approval.
Do not repeat the old claim that no native Makalu integration has occurred.

The same workspace retains the failed 50M benchmark, accepted R4/R5 diagnostics
and R6.1 stop as distinct historical results. Local packaging/fuzz improvements
do not change those dispositions or authorize another remote workload.

## Ownership and next concrete actions

- Engineering: finish required local source/tests/release documentation and
  deliver exact source/artifact pins. Keep LAX and MultX changes out of this work.
- Standards owners/Foundation: supply accepted LEP100-15 canonical signing rules
  and the required production profile. Existing unresolved choices cannot be
  invented by the implementation and called interoperable.
- Foundation technical approver/operators: decide the pending bounded diagnostic
  continuation and final workload/economics acceptance using actual evidence.
- Independent reviewer/release owner: review the final delta and evidence,
  establish the signing identity and approve publication/rollout.

No new policy questionnaire or deployment approval is generated by this record.
Existing approvals remain scoped to their exact source, operation and window.
