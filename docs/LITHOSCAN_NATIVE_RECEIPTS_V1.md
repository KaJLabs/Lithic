# Native deployment receipt handoff

Local candidate, 2026-09-26. Not a deployed RPC or explorer feature.

`DeploySuccess.deployments` and `HostSuccess.deployments` contain host-generated
`CommittedDeployment` records, separate from contract events. Each records
creator, contract, code hash and either transaction nonce or child template/salt.
After the LTH-R1 caller-binding remediation, child `salt` is the effective salt
from `caller_bound_salt(immediate_caller, user_salt)`, not the user input. Indexers
feed it directly into `child_contract_address`; do not hash it a second time.
They are accumulated in registration order (parent before initializer children)
and returned only after the host state transaction commits. Failed initialization,
parent revert, out-of-gas, collision and commit failure return no success journal.
`TokenCreated` emitted by arbitrary contract code cannot create a registration.

The journal is capped at 64 registrations per host transaction; top-level
deployment occupies one slot. Exceeding the cap aborts and discards all writes.
This is a candidate resource bound, not approved consensus policy.

`lithovm_host::deployment_status::included` maps that journal plus an `Inclusion`
context into `sdk/schemas/lithovm.included-deployment.schema.json` observations.
It rejects duplicate addresses, oversized journals, missing chain/transaction/block
identity, noncanonical addresses and mismatched address derivations. The entire
batch rejects on any bad record. Chain IDs, heights, nonce and gas are decimal
strings to preserve u64 precision for JavaScript clients. `transactionGasUsed`
is transaction-wide, not falsely attributed to each creation.

The deployment ID binds chain, block hash, transaction hash and creation index.
Re-inclusion on another branch has a different ID. The result is always
`included` and `unverified`; it is not finality or source verification. This
observation schema is separate from the earlier generic deployment-status v1
schema and does not silently change its numeric field types.

## Required chain/indexer integration

The caller must obtain `Inclusion` from an authenticated canonical chain source
after the **entire outer transaction** succeeds. A native host commit into an EVM
cache is not inclusion: outer revert must discard native state, logs and this
journal. The Rust mapper does not authenticate RPC responses or establish finality.
Do not populate it from user POST bodies or contract-emitted token events.

The inspected Lithosphere `Makalu/indexer/src/mappings.ts` currently consumes
block/EVM observations and writes PostgreSQL. No approved native receipt source
exists to connect here yet. Its native integration must atomically persist the
batch with the corresponding block, invalidate orphaned observations, retain
failed transaction status separately and avoid manufacturing addresses for
failed creation. Publication remains disabled until that adapter exists.

For verification, read native code at the same trusted block hash/height, then
run the existing pinned `lithverify` worker with exact source and artifact. Bind
its result to chain/address/code hash/compiler identity before recording source
match. A source match is not finality or audit approval. Worker isolation,
compiler provenance and persisted verification/status views remain required.

## Local evidence

Creation tests check host records, top-level/child ordering, actual status mapping,
fake-event rejection and rollback when the 65th registration exceeds the cap.
Status tests check exact u64 serialization, branch-specific IDs, wrong-chain,
tampered hash, duplicate records and missing block identity. The new cap test
initially used unsupported inline hex operands; it was corrected to use typed
bytes32 constants, matching the supported source subset.

Validation on this candidate: `cargo test --workspace --quiet`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo check --manifest-path fuzz/Cargo.toml --bins` passed in WSL/Linux.
No new fuzz campaign or live explorer integration test is claimed for this change.

No live LithoScan database, RPC, frontend, chain or MultX code was changed.
