# Synchronous native calls: local v13 candidate

Status: local compiler/VM/transactional-host implementation, not preview.3,
not a production release, and not deployed on Makalu or mainnet.

## Source interface

```lithic
contract Caller {
    pub fn run(target: address, selector: bytes32, number: u64) -> u64 {
        let answer: u64 = invoke(target, selector, 0, number);
        return answer + 1;
    }
}
```

`invoke` is an immutable, explicitly typed local binding statement, not a
general expression. Its first three operands are native address, full 32-byte
canonical function selector and attached `u256` native value. Zero to 64 typed
arguments follow. The caller's annotated result type must match the callee.
Argument count/types are checked by the callee's VM before its body runs.
Integer argument literals default to `u64`; use typed constants/locals when
passing `u256`. The attached-value operand accepts an in-range numeric literal.
There is no EVM selector translation or ERC-20 interoperability.

The caller suspends, the child executes, then its typed result becomes the local
binding. Events and native transfers in v13 frames execute in source order.
Child failure, wrong type, unknown selector, missing contract, insufficient
balance, output limit, reentrancy or gas exhaustion aborts the entire transaction.
Failures cannot be caught. A successful child does not commit independently:
subsequent caller failure also discards its storage, balances and events.

The child sees the calling contract as `msg.sender`, the attached amount as
`msg.value`, the same block/chain context, and incremented call depth. The
existing depth limit applies and any call to an active ancestor is rejected.
The caller's staged storage is published to the shared transaction before
entering a child. On return its balance is refreshed, including child refunds.

## Encoding and compatibility

A program containing any `invoke`, including nested control flow, encodes as
`lithovm-native-v13`. Other programs continue to emit v11 or v12. Statement tag
14 contains return-type byte, target/selector/value expressions, big-endian
u16 argument count, then each type byte and expression. Existing expression
encoding and type tags are unchanged. Older-version headers reject this tag.
Programs mixing `invoke` and deferred `call_contract` are rejected, even if the
operations occur in different functions. This is a program-wide execution mode.

v13 callers can invoke deployed legacy v11/v12 contracts through the typed host
interface. Legacy frames retain their previous deferred transfer/call ordering;
their complete deferred subtree finishes before the v13 caller resumes. Do not
interpret this compatibility as retroactively changing legacy ordering.

Use `TransactionalHost::deploy_values` / `execute_values`, including for
scalar-valued v13 programs. Scalar host entry points do not execute v13.
Unhosted VM execution also rejects v13. The artifact schema accepts matching
v13 target/version pairs; source/code hashes and selector derivation are unchanged.

## Transaction and gas interface

The VM's `execute_hosted(ValueCall, Storage, ExecutionHost)` seam exposes only
event emission, immediate native transfer and invocation. `lithovm-host` supplies
the adapter; it uses the existing outer `StateTransaction`, event journal and
active-contract stack. A custom adapter must stage **all** external effects and
discard them on any VM failure. Calling `execute_hosted` alone does not make an
external database transactional. The host result's transaction-wide events are
authoritative; the individual VM result contains only that frame's own events.

Candidate call gas is one statement charge plus existing `CONTRACT_CALL_GAS`
(40), operand expression charges and string-copy byte charges. The child receives
the remaining gas and its aggregate consumed gas is charged to the parent on
success or failure. Return-string copying is charged before allocation. Child
ingress and egress retain the v12 schedule. Out-of-gas consumes the supplied
limit; other failures retain consumed gas and identify the failing contract.
This is not a consensus-approved chain fee schedule. Storage persistence, code
loading, address resolution and host allocation costs still need chain pricing
and DoS review.

Existing dynamic limits apply: 4096 bytes per string, 64 values per envelope,
65536 bytes per argument envelope and per transaction's event-value envelopes.
Gas bounds execution; event caps apply across caller and child frames.

## Acceptance and remaining work

`packages/vm/lithovm-host/tests/synchronous.rs` exercises typed return data,
source-ordered events/transfers, refunds, exact and all insufficient gas budgets
of a representative call, parent/child failure rollback, selector/type/count
errors, reentrancy, missing contracts, atomic failed initialization, UTF-8 and
maximum-sized strings, three-level calls, event-cap failure/recovery, downgrade
and every-prefix truncation rejection. `synchronous_calls` fuzzes gas, amount,
integer arguments and parent rejection against state/balance/event invariants
and deterministic replay.

Not implemented here: contract factory creation, EVM calls, catch/reentrant
profiles, source string literals, general arrays, persistent consensus adapter,
authenticated native transactions/RPC, native wallet signing or live deployment.
Independent security review, longer campaigns and Makalu evidence remain open.
No LAX/production deployment or MultX change is authorized by this candidate.
