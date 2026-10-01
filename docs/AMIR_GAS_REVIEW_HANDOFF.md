# Amir: Foundation technical gas review

Prepared 2026-09-27. Recipient: **@Amir Dev**, nominated by Alex as technical
approver for **Lithosphere Foundation**, per the user's relayed confirmation.
No GitHub handle or messaging destination is inferred from this display name.

## Scope

Review the disabled storage/log gas candidate implemented in `a64703f`.
The preceding caller-bound salt and mandatory atomic-initialization changes
(`b06c17e`) implement Alex's confirmed policy. Their independent audit retest
is still outstanding. This handoff asks for gas-policy decisions and validator
acceptance criteria; it is not a claim of audit closure or production readiness.

Read [the proposal](LTH_R1_GAS_SCHEDULE_PROPOSAL.md) first, particularly its
consequences and limitations. [Raw optimized benchmark output](evidence/LTH_R1_GAS_RELEASE_BENCHMARK_2026_09_27.txt)
and [the audit tracker](LTH_R1_REMEDIATION.md) distinguish local evidence from
outstanding independent tests.

## Requested response

Please return the following, referring to the package's source commit:

1. **Numerical candidate policy:** accept, revise, or defer each rate below.
   Acceptance applies to the disabled review candidate only.
   - Reads: 1,000 + 3 gas per key/value byte, for each physical Get.
   - Changed writes: 2,000 + 30 gas per key/new-value byte.
   - Growth: 20,000 gas per additional rounded 32-byte key/value unit; no refund.
   - Actual envelope logs: 750 + 8 gas per emitted data byte.
   - Aggregate native budget: 10M, including VM, reads, writes and logs.
2. **Required workloads:** minimum deployable contract size, expected token-holder
   and allowance counts, peak events/transaction, and acceptable transaction cost.
   If product requirements already exist, a link is sufficient. The proposal's
   growth rate permits at most 15,264 newly allocated serialized bytes under 10M
   before other charges; the current whole-record layout becomes more expensive
   as map state grows. Do not approve this as a scalable token layout by default.
3. **Validator acceptance criteria:** test-machine specification/owner, current
   block gas and target execution-time budget, and allowed persistent growth.
   Provide an isolated benchmark environment or nominate an operator to run it;
   do not send credentials in this response. No live chain activation is requested.

Suggested reply format:

```text
Source commit reviewed:
KV read/write rates: accept / revise / defer (details)
Growth rate and 10M aggregate cap: accept / revise / defer (details)
Log rates: accept / revise / defer (details)
Workload requirements or reference:
Validator benchmark operator/environment and acceptance budgets:
Scope: disabled remediation candidate only; no deployment/activation approval.
```

## Engineering work that remains ours

We must implement/review shared live cross-language fuel accounting, scalable
per-key persistence and native store lifecycle/genesis/upgrade handling, then
rerun relevant tests and obtain independent retesting. A pricing approval does
not waive these items. Local microbenchmarks exclude durable disk commits,
consensus and validator-class full-block tests. The full R1 replacement audit
package and aligned source pins must follow completed remediation; the original
R1 package remains unchanged.

## Attachment contents and reproduction limits

The companion ZIP is a **targeted technical decision packet**, containing this
handoff, the proposal, raw benchmark output, tracker, and the tracked Go/native
integration source/tests from one Git commit. It is not the complete toolchain,
an offline build bundle, a signed release or the replacement security submission.
The proposal gives the Rust/Evmos/SDK pins and commands needed with those source
trees and dependencies. Benchmark Rust code is unchanged from `b06c17e`.
No binaries, secrets, production deployment scripts or MultX files are included.
