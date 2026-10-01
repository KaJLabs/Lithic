# Independent security review handoff: Lithic/LithoVM candidate

Status: ready for **independent review intake**, not a security sign-off,
production release, Makalu activation or LAX deployment approval. Both review
PRs remain drafts: [Lithic #13](https://github.com/KaJLabs/Lithic/pull/13)
and [disabled Lithosphere overlay #223](https://github.com/KaJLabs/Lithosphere/pull/223).
The compiler/runtime code reviewed locally for this handoff is pinned at
`d3d00cad7e6c825e5f97e6b143037c9c0ff2eced`; reviewers should record
the exact PR heads they assess and request a delta review if either moves.

## Scope and source chain

- Baseline: `lithic-v0.2.0-preview.3`; the [gap matrix](PRODUCTION_GAP_MATRIX.md)
  distinguishes implemented candidate behavior from missing production evidence.
- Candidate compiler/runtime: typed storage/maps, events, transfers, rollback,
  typed synchronous native calls, salted child creation, LEP100 token vectors,
  deployment records and strict gateway ABI. The native-to-EVM path and
  LEP100-15 profile are **not implemented for production**.
- Disabled chain overlay: Evmos `eca13ef2521a9ef13c32e80b1b147230bdb155b5`
  plus the existing SHA-pinned Lithosphere release patches and the isolated
  overlay in PR #223. Its manifest pins the overlay SHA-256 and Lithic dependency
  `4de97ab806ebec5b05c4eeab238281320b973f65`. The later Lithic host
  refactor does not alter that gateway dependency. Ordinary builds install no
  native gateway; `lithovm_chain_lab` installs an active **test-only** gateway.
  Never deploy a tagged binary.
- The separate local Evmos lab commit is recorded in the overlay manifest; the
  independently reviewable source is the patch against the public pinned Evmos
  commit. Tagged tests still require a local Go-module replacement as documented
  in the overlay README. This is not a hermetic signed chain release.
- The Counter test bytecode was compared with `lithc --emit lithovm` output for
  the pinned source. Its `sourceHash` is Keccak-256, not file SHA-256; the
  overlay labels it correctly and pins the patch's separate SHA-256. The
  Counter file SHA-256 is `0fbdf538ae97c87283d26e0ca156cc5d20dd95fbea155181389176e6f2c17be4`;
  compiler `sourceHash` is `ac549823aa139c6df36515d31a9e35a2959187998997f060d9a57e18f57c5c28`
  and `codeHash` is `581e79fb3c20653dbd65369bc8887254d000fe44fee90b81175bee80ccd6c056`.

## Highest-risk review questions

1. Can untrusted bytecode, selectors, dynamic values or JSON/FFI input cause
   noncanonical decoding, unbounded allocation, panic, or compiler/VM
   interpretation drift? Review bytecode versioning and source verification.
2. Do storage, balances, events, child contracts and deployment records all
   roll back across nested failures, out-of-gas, EVM parent revert, simulation
   and process restart? Check StateDB snapshots and bank/module-account guards.
3. Can a wrapper, fake message, replay, or mismatched chain ID/nonce impersonate
   a direct wallet deployer? The keeper lab signs a local message and calls
   `ApplyMessageWithConfig` directly; it does **not** prove ante verification,
   Makalu broadcast, replay protection or finality.
4. Are the EVM gateway ABI, selector identity, child salt/address domain,
   EVM/native address collisions, call depth and reentrancy policy safe?
   Specifically test a wrapper call after another precompile changes the map.
5. Is the provisional gas schedule safe under worst-case calldata, storage,
   logs, recursion and failure? There is no approved consensus gas schedule.
6. Can receipts, source verification or frontend status claim a deployment
   before committed/finalized chain state? Review trust and finality assumptions.

## Reproduction and evidence

- Rust: `cargo +1.96.0 fmt --all -- --check`,
  `cargo +1.96.0 clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo +1.96.0 test --workspace --locked`. Inspect the PR #13 cross-platform
  and five fuzz-target CI results and [bounded fuzz evidence](FUZZING_EVIDENCE_2026-09-26.md).
- Chain: follow PR #223's `infra/lithovm-disabled-candidate/README.md` to apply
  the pinned overlay. Build `cargo +1.96.0 build --locked -p lithovm-ffi` from
  the Lithic checkout, then run ordinary and tagged `go test -mod=mod -count=1
  ./x/evm/keeper`, plus tagged candidate race tests. The local pinned Go
  1.22.12 keeper suites passed after the review fixes. See
  [keeper lab evidence](NATIVE_CHAIN_KEEPER_LAB_2026_09_27.md).
- Review findings already corrected: shared selector-collision validation has
  a direct duplicate-entry test; the keeper fixture explicitly chooses its
  funded wallet sender; the Counter source digest is labeled Keccak-256.
- CI and local tests are conformance evidence only. Independent longer fuzzing,
  dependency/SBOM review, threat modelling and manual cross-runtime review
  have not been completed by an external reviewer.

## Required reviewer output and release gates

For each finding, record affected commit/file, severity, reproduction,
impact, remediation and independent retest result. Security acceptance should
explicitly state the reviewed commits and residual risks. Chain-owner approval
of consensus identity/ABI/gas/rollback, signed Makalu deploy/call/failure and
recovery receipts, a reproducible signed release plan, frontend/LithoScan
finality integration and explicit deployment approval remain separate gates.
Do not merge this review package as production-ready or deploy LAX/production
contracts on the strength of this handoff. MultX is out of scope.
