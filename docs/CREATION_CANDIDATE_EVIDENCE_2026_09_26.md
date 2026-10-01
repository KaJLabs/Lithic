# Child-creation candidate evidence, 2026-09-26

Scope: local changes based on `10574c8`, not preview.3, a production release,
security approval or a deployed chain feature. See [v14 semantics](LITHOVM_CREATION_V14.md).

Completed in WSL/Linux:

- `cargo test --workspace --quiet`: passed, including five new creation tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings`:
  passed for the separate fuzz workspace.
- `cargo fmt --all` and fuzz workspace formatting completed.
- Workspace/fuzz formatting checks and `git diff --check`: passed.
- `cargo +nightly fuzz run child_creation -- -max_total_time=20 -timeout=5 -max_len=64`:
  exit 0, seed 3498773128, 35,960 runs in 21 seconds, no reported crash.
  The generated on-disk corpus retains 23 files. The target varies gas, native
  value, salt, prefunding and child/parent acceptance, checks deterministic
  replay and modeled code/state/balance/event results, and retries successful
  creation to verify collision rejection and unchanged state.

Tests additionally cover all insufficient gas budgets of one representative
creation and exact-gas success; false initializer and late parent failure;
prefunded balance preservation; missing template, selector/type/argument errors,
insufficient funds and child code-size limit; immediate invocation of a newly
created child; top-level deployment rollback/retry removing both factory and
child; and downgrade/truncation/unsupported-source rejection.

The finance factory test creates children for all 16 feature combinations from
an already initialized v12 token template. It verifies exact Unicode/NUL metadata,
6 decimals, supply, creator ownership (or renounced ownership), creator balance,
zero factory token balance, event attribution and unchanged template state.
Existing token behavior tests continue to run; this does not establish complete
OpenZeppelin differential equivalence or live frontend integration.

The compiler corpus has a named `child_creation.lithic` seed. CI configuration
includes the execution target; no hosted CI run is claimed. This short campaign
does not cover arbitrary child graphs, dynamic initializer arguments, persistent
state faults or consensus execution and does not replace independent review.

No chain activation, deployment, frontend publication, GitHub push, LAX change
or MultX modification was performed for this milestone.
