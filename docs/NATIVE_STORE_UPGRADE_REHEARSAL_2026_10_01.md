# Native-store upgrade rehearsal — disabled lab, 2026-10-01

Status: **engineering candidate only**. This is not a Makalu governance plan,
registration, gateway activation, production pricing approval or permission to
deploy contracts. The lab source is Evmos commit
`badff8be1a3bb565923ae63fd4f19599da4394d6`, based on the separate-store
candidate `24e674de1bd4150025562aa6e393a672736feb65`.

## Candidate mechanism

The `lithovm_chain_lab` build registers a distinct
`lithovm-native-store-lab-v1` SDK upgrade handler. Only a disk plan with that
exact name and a positive height configures `UpgradeStoreLoader` to add the
`lithovm` KV store at the scheduled height. The ordinary build has no native
store, no native upgrade handler and no loader. The lab handler runs normal
module migrations; it does not enable the gateway or seed native contracts.
The plan name is deliberately not a proposed Makalu production name.

On the isolated VPS, the focused app test rehearsed an existing EVM store at
version 1, rejection of an unrelated or zero-height plan, rejection of a
wrong-height/default load with the new store, addition at height 2, preservation
of existing EVM state, an initially empty native store, and persistence of
native state after commit and reopen. The tagged app/keeper suites and ordinary
app/keeper suites passed before the final legacy-reopen observation was added;
the focused test passed again afterward. Commands:

```text
go test -mod=mod -tags lithovm_chain_lab -run '^TestLithoVMStoreUpgradeIsHeightAndPlanGated$' -count=1 ./app
go test -mod=mod -tags lithovm_chain_lab -count=1 ./app ./x/evm/keeper
go test -mod=mod -count=1 ./app ./x/evm/keeper
```

## Mixed-version and rollback boundary

The rehearsal found a **critical operational limitation**: an old binary that
mounts only its original stores can reopen a post-upgrade database containing
the new native store. The SDK store loader alone does not make an old binary
reject that database or prevent mixed-version block execution. The legacy
reopen is recorded in the focused test, not represented as a protection.

The exact cause is the pinned SDK `rootmulti.Store.loadVersion`: it builds the
persisted `StoreInfo` map, then loads only keys in the binary's mounted
`storesParams`. It does not reject a persisted store absent from that mounted
set. `UpgradeStoreLoader` is called only by the new binary. Changing the new
candidate or its SDK fork cannot retrofit a check into the already-deployed
legacy executable. A new-binary-only guard would therefore be insufficient
evidence of mixed-version protection.

Consequently, the following must be part of a Foundation-reviewed plan before
any Makalu candidate is scheduled:

1. Pin the exact candidate binary, app hash expectations, plan name and height;
   verify all validators have the matching candidate and a tested halt/restart
   procedure before that height. No rolling mixed-version operation through the
   upgrade block.
   If a pre-upgrade guard binary is chosen, it must be rolled out and verified
   on **every** validator before the store-addition height; any still-running
   older executable defeats that guard. This rollout is a separate approved
   stage, not a property of the lab loader.
2. Capture a restorable state snapshot **before** the upgrade height and
   rehearse restoration on the isolated chain. A post-upgrade database must not
   be handed to an old binary as a rollback strategy.
3. Require explicit operator checks that all nodes have crossed the same
   upgrade height and loaded the new store before native registration is even
   considered. Keep the gateway disabled during this phase.
4. Test an interrupted upgrade, wrong/missing plan, wrong binary, state hash
   continuity, restart and restore-from-pre-upgrade-snapshot on a networked
   Makalu-equivalent rehearsal. A single in-memory root-store test is not that
   rehearsal.

If Foundation cannot enforce the coordinated halt and pre-upgrade snapshot
rollback boundary, this store-addition path should not be proposed. An
alternative would require a separately designed version guard that existing
validators can enforce; the present code does not supply one.

The engineering task cannot honestly be marked complete merely by making the
lab test green: the vulnerable behavior belongs to an immutable deployed
binary, and the durable/networked rehearsal and Foundation cutover decision
have not occurred. No live node or systemd service was changed to mask this
gap.

## Open approvals and measurements

Foundation must choose and approve the actual upgrade name/height and operator
cutover and rollback runbook. Alex has accepted the isolated
13-vCPU/78-GB/350-GB VPS for benchmarking despite the original 500-GB
nomination; this does not approve its results or final pricing. The
[durable 1k/10k/100k holder and allowance run](DURABLE_TOKEN_STATE_BENCH_2026_10_01.md)
and [deploy/log boundary smoke](DURABLE_BOUNDARY_CANDIDATE_2026_10_01.md)
are complete as disabled isolated evidence, not validator acceptance.
Full-block measurements through the live 100M block-gas ceiling, pricing
policy, independent security retest and Makalu end-to-end tests remain
outstanding. The test chain and lab upgrade do not authorize LAX or any
production deployment.
