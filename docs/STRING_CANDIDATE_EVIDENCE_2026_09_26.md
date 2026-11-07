# Local string candidate evidence — 2026-09-26

Scope: local compiler/VM candidate built on `08d4037`, not preview.3, a signed
release, a security approval or a deployed chain capability.

Completed checks in WSL/Linux:

- `cargo test --workspace`: passed, including five compiler-to-VM string tests
  plus map-only stateless-call rejection and scalar-host dynamic-deployment
  rejection regressions.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all` and `git diff --check`: passed.
- `cargo +nightly fuzz run compiler_runtime -- -max_total_time=20 -timeout=5 -max_len=8192`:
  exit 0; seed 2119651294; 38,428 runs in 21 seconds, no reported crash.
  Initial corpus included the new `string_metadata.lithic` seed. This target
  checks deterministic compilation, artifact/version agreement and VM loading;
  it does **not** fuzz dynamic execution, prove coverage or replace review.
  The 447 generated corpus cases are retained locally under ignored
  `fuzz/artifacts/compiler-runtime-20260926-corpus/`; the named source seed is
  retained in the versioned corpus. No corpus cases were deleted.

Tests exercise exact Unicode/NUL/empty/4096-byte values; compiler locals,
storage, events and returns; equality without normalization; gas by UTF-8 byte;
all insufficient gas budgets of a representative stateful call; revert and
recovery; oversized arguments/output; downgrade/truncation rejection; and
continued v11 scalar output. Source literals, factories, ordered calls and
native chain execution remain unimplemented. See
[the candidate specification](LITHOVM_DYNAMIC_VALUES_V1.md).

## Dynamic host follow-up

Explicit `deploy_values`/`execute_values` now share the scalar host's transaction
engine. Three additional host tests cover atomic Unicode initialization and
prefunding, every insufficient gas budget for deployment, initializer revert,
collision, child revert/OOG, transaction-wide event limits, recovery, and the
v12 finance-token metadata fixture across 16 initialization profiles. No
contract-level calldata/return-data channel or factory creation was added.
The compiler fuzz smoke above predates this host-only follow-up and is not
evidence of host fuzz coverage.

No LAX or production deployment, live RPC change, or MultX modification occurred.
