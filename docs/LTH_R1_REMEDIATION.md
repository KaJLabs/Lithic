# LTH-R1 remediation tracker

Report received 2026-09-27. DOCX SHA-256 verified:
`08e3a54b757d4bc9e0c3868f90d07f141c720df4778d3d867ed5576545fa00be`.
Reviewed submission: Lithic `32bf868`, Lithosphere overlay `511b63c`.
All audit findings remain open until independent retest. No registration,
activation, LAX or production deployment is authorized.

Amir's technical review is recorded in [the response and evidence matrix](AMIR_GAS_REVIEW_RESPONSE.md):
KV/log rates accepted for the disabled candidate; growth and 10M final policy
deferred. Reviewed source `fcb2f47` directly descends from `a64703f` with identical
native-chain implementation/tests. Final one-pin closure remains pending further
engineering. The earlier `max_gas=-1` observation was **mainnet** (chain
`lithosphere_9005-1`), not Makalu. A later [read-only capture from the active
Makalu validator](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md)
measured a 100M consensus block-gas ceiling at height 16,237,322. It is
single-node evidence; do not transpose either chain's limits to the other.

| Finding | Remediation / remaining evidence |
| --- | --- |
| LTH-01 | Alex's remediation approval relayed by user on 2026-09-27. Host now derives effective salt from domain tag, immediate caller and user salt. Two-user FinanceFactory ownership/collision and receipt-reconstruction tests pass locally; independent retest remains open. |
| LTH-02 | Candidate clamp forwards min(EVM frame gas, 10M) to Rust while charging actual work. Regression first failed at 10M+1/15M/30M with FFI code 2 and zero gas left; passes after clamp. Keeper boundary and RPC-facing EstimateGas tests pass locally (details below); HTTP JSON-RPC/Makalu and independent retest remain outstanding. FFI's direct-request safety cap remains intact. |
| LTH-03 | Disabled candidate implements pinned SDK KV rates, EVM LOG1 rates and proposed growth charges, with early read charging, unchanged-write suppression and rollback tests. Optimized local benchmark (5,400 calls) and keeper tests pass. [Shared live fuel and per-key state](NATIVE_PER_KEY_FUEL_CANDIDATE_2026_09_30.md), [durable state workloads](DURABLE_TOKEN_STATE_BENCH_2026_10_01.md), and [deploy/log boundaries](DURABLE_BOUNDARY_CANDIDATE_2026_10_01.md) now have isolated candidate evidence. Final growth/cap policy, full-block validator evidence and independent retest remain outstanding; not closed. |
| LTH-04 | Approved atomic-initialization policy implemented for top-level and child deployment. Missing/wrong entrypoints fail without publication; FinanceFactory takeover, FFI rejection and existing rollback/recovery regressions pass locally. Independent retest remains open. |
| LTH-05 | Four Rust guard tests added; independently disabling M4, M5, M6 and M8 makes each corresponding test fail. Guards restored; independent auditor retest still required. |
| LTH-06 | After code fixes, freeze one Lithic commit and rerun FFI/Go/keeper evidence at that pin; update overlay and immutable handoff links together. |
| LTH-07 | Build next package on Linux with forward-slash paths and Git bundles; verify raw blobs and git fsck. Preserve R1 archive unchanged. |
| LTH-08 | Carry explicit wallet fixture into L1 release evidence and require full keeper suite in the next candidate's release gate; do not rewrite frozen r1 artifacts. |
| LTH-09 | Reads and writes share canonical native-key validation, including the Go callback before invoking its Reader. A disabled separate-store/per-key candidate and [height-gated upgrade rehearsal](NATIVE_STORE_UPGRADE_REHEARSAL_2026_10_01.md) exist. Coordinated validator cutover, durable networked rollback/restart rehearsal, chain acceptance and independent retest remain open before registration. |

LTH-02 regression command (Linux, Go 1.22.12, existing pinned SDK/Evmos lab
replacements and built Rust FFI required):

```sh
cd integration/native-chain
go test -mod=mod -tags lithovm_chain_lab -run TestFrameGasAboveNativeCap -count=1 .
go test -mod=mod -tags lithovm_chain_lab -count=1 .
```

The added EVM.Call test compares return bytes and gas consumed across 10M-1,
10M, 10M+1, 15M and 30M budgets, using fresh transaction contexts. This is not
an RPC estimateGas or Makalu test. The reviewer could not reproduce Go/fuzz
campaigns in R1; vendor passes must remain distinguished from independent
evidence. The review also recommends a dedicated compiler/VM audit round.

## Keeper estimation follow-up

Local chain-lab commit `45d051dc1fe2f0e9e585b09be50c1df80ec99e24`
adds boundary and estimation checks to
`TestLithoVMCandidateSignedDeploySimulationAndCommit`. The focused tagged
keeper test and full tagged keeper suite pass with Lithic gas-clamp code at `22d9b78`:

- Signed core messages at 10M-1, 10M, 10M+1, 15M and 30M preserve successful
  output and do not burn the entire allowance.
- Keeper `EstimateGas` (the RPC-facing method) succeeds at caps 10M, 30M and
  50M, both with and without an explicit gas argument. Replaying its returned
  gas estimate succeeds, and estimation does not persist the contract.
- Keeper receipt gas includes Evmos's existing minimum-gas multiplier and is
  therefore not constant as the transaction limit changes. This was not
  modified. The bridge-level work charge remains constant for the same work.

This is direct keeper invocation, not HTTP JSON-RPC or a Makalu broadcast.
The chain overlay and final review bundle will be re-pinned together after
the remaining remediations; the immutable R1 submission remains unchanged.

## LTH-05 mutation evidence

Each mutation below was applied alone, its focused test was executed and failed
on `unwrap_err()` receiving success, and the original guard was restored before
the next case. This is vendor reproduction, not independent audit closure.

| Mutant | Regression test in lithovm-ffi | Result when guard disabled |
| --- | --- | --- |
| M4 stored code hash | stored_hash_guard_rejects_well_formed_record | FAILED: accepted valid JSON with wrong stored hash |
| M5 payable balance | payable_guard_rejects_nonzero_balance_without_commit | FAILED: accepted nonzero balance |
| M6 aggregate writes | aggregate_write_guard_accepts_limit_rejects_one_byte_over | FAILED: accepted one byte above aggregate cap |
| M8 read count | read_count_guard_rejects_257th_read | FAILED: accepted 257th read |

The aggregate-write test supplies staged serialized byte accounting directly;
it isolates the aggregate limit from record-count and per-record limits. It
does not claim end-to-end contract execution producing that batch. The hash
test starts from a valid stored contract and changes only its stored hash.

## Chain-owner decisions and scope

The user reports Alex confirmed points 1 and 2 on 2026-09-27. The client identifies
Lithosphere Foundation as chain owner and explicitly nominates @Amir Dev as
technical approver. No numerical gas schedule, security acceptance, deployment
or activation approval is inferred from those confirmations.

1. Approved caller-bound public-factory salt derivation:
   derive an effective salt from a domain tag, authenticated immediate caller
   and user salt, before the existing child-address derivation. This changes
   predicted factory addresses and requires matching SDK/frontend calculation.
2. Approved rejecting deployment without atomic initialization for contracts
   declaring `initialize`. This is the auditor's alternative to storing a
   deployer identity. It changes the currently allowed zero-initializer profile;
   templates declaring initialize must also be initialized atomically.
3. @Amir Dev accepts the candidate KV/log rates for disabled remediation only.
   Final growth pricing and the 10M cap are deferred. Scalable per-key storage,
   live fuel accounting and validator-class durable/full-block evidence remain
   required before final economics or registration approval. See the recorded
   review for workload sizes, measurements and outstanding operator inputs.

These decisions define the remediation candidate, not permission to deploy
or activate it. LTH-06 source repinning and LTH-07 replacement packaging should
follow the profile fixes, not freeze another incomplete audit submission.

## Approved-profile regression evidence

`cargo +1.96.0 test --locked -p lithovm-host --test approved_profile` first
failed both original regressions: accepted missing initialization and copied salt
blocked the victim. After the fix, the suite also covers a bool-returning bypass
selector, actual FinanceFactory takeover rejection and indexer reconstruction.
The existing FinanceFactory/token test now checks two users sharing salt 0 and
the second user's ownership, plus same-caller collision and unchanged state.

Full Rust workspace tests and Clippy with warnings denied pass. The tagged native
Go suite passes against the rebuilt FFI, including omitted/bypassed initialization
and StateDB creation rollback. The full tagged Evmos keeper suite also passes
locally (22.262s), including the prior signed-message/estimation regressions,
against chain-lab commit `45d051dc1fe2f0e9e585b09be50c1df80ec99e24`.
This is not HTTP RPC, Makalu, independent retesting or chain acceptance.
The child-creation fuzz harness now initializes
its template atomically and predicts with the caller-bound salt. A local seeded
run (`cargo +nightly fuzz run child_creation -- -max_total_time=30
-seed=20260927 -max_len=64`) completed 11,828 executions in 31 seconds, no failure.
This is a short regression campaign, not production fuzz coverage or audit closure.
The fuzz lockfile was refreshed to include the VM's existing serde dependencies.

The Rust `caller_bound_salt` helper and creation/receipt docs define prediction.
No native child-address prediction implementation currently exists in the Finance
frontend service: native submission still fails closed. Its six service tests
pass unchanged. Future native frontend wiring must use the effective salt and
must not bind it twice. No Finance production deployment was changed.
