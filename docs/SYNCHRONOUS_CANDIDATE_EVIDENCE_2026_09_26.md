# Synchronous-call candidate evidence, 2026-09-26

Scope: local changes based on `346d1a4`, not preview.3 or a released/approved
production capability. See [semantics and limits](LITHOVM_SYNCHRONOUS_V13.md).

Completed in WSL/Linux:

- `cargo test --workspace`: passed, including eight new synchronous host tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings`:
  passed for the separate fuzz workspace.
- `cargo fmt --all`: completed.
- Workspace/fuzz formatting checks and `git diff --check`: passed.
- `cargo +nightly fuzz run synchronous_calls -- -max_total_time=20 -timeout=5 -max_len=64`:
  exit 0, seed 3860379889, 41,286 runs in 21 seconds, no reported crash.
  The retained corpus contains 20 generated files (the fuzzer reports 21 corpus
  entries including its initial entry). This target executes two
  deployed in-memory contracts with varying gas, native value, integer argument
  and parent rejection. It checks deterministic replay, rollback, gas bounds,
  modeled balances/storage/return data and source-order events. It does not
  cover arbitrary call graphs, strings, persistence, EVM or chain execution.

Unit tests additionally cover child failures, missing/unknown/mismatched calls,
reentrancy, maximum-sized strings, three frames, native refunds, aggregate event
limits/recovery, failed deployment initialization, malformed bytecode and every
insufficient gas budget of one representative call. A native token-pull test
uses the existing v12 finance-token fixture: missing allowance fails, approval
allows a pull, allowance/balance updates match, caller rejection restores token
state and a subsequent over-allowance pull fails atomically. This is not an
EVM ERC-20 call or factory creation. These are bounded checks,
not proof of completeness or independent security review.

The compiler/runtime corpus includes a named `synchronous_call.lithic` seed.
CI configuration includes the new execution target and host source path; this
does not claim a hosted CI run has occurred.

No LAX/production deployment, chain activation, frontend publication, GitHub push
or MultX modification was performed for this milestone.
