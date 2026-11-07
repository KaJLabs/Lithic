# Atomic child creation: local v14 candidate

Status: compiler/VM/in-memory-host implementation, not preview.3 or an approved
consensus feature. No native Makalu/mainnet deployment path is established.

## Interface

```lithic
contract Factory {
    pub fn create(template: address, salt: bytes32, initializer: bytes32, name: string) -> address {
        let child: address = create_contract(template, salt, initializer, 0, name);
        return child;
    }
}
```

`create_contract` is an immutable, explicitly `address`-typed local binding.
The fixed operands are a deployed native code-template address, `bytes32` salt,
full 32-byte initializer selector and attached `u256` native value. Up to 64 typed
initializer arguments follow. String and envelope limits are unchanged from v12.
Only code is copied: child storage starts empty/zero, independent of template
storage and balances. The template's stored code hash is verified and the child
entrypoint map is freshly derived from validated bytecode.

The initializer is mandatory, must return `bool`, and must return `true`.
If the program declares `initialize`, that exact entrypoint must be selected;
another bool-returning function cannot bypass it. Top-level deployments of such
programs (including templates) also require atomic `initialize` execution.
`false`, revert, invalid arguments, unknown selector and all other failures abort
the entire outer transaction. Its caller is the factory contract, not the wallet;
the factory must explicitly pass the original creator where required. Its value
is the attached amount. Initialization shares the outer event journal, remaining
gas, call depth and reentrancy policy. The returned address can immediately be
used by `invoke` in the same frame.

Code, initial state, value, parent writes and events commit together. Later parent
failure removes the staged child too. No catch/partial-success mode is provided.
Top-level factory deployment may also create a child in its initializer; failure
then discards both deployments. Existing native balance at a not-yet-deployed
child address is preserved and credited, never overwritten.

## Candidate identity and limits

Following Alex's remediation approval reported on 2026-09-27, the host binds
every creation salt to the immediate authenticated caller of the creating frame:

```text
effective_salt = keccak256("LITHOVM_CALLER_SALT_V1" || caller_word32 || user_salt32)
```

`caller_bound_salt(caller, user_salt)` exposes that calculation for clients.
`child_contract_address(creator, effective_salt, code_hash, chain_id)` returns the canonical
32-byte word holding the low 20 bytes of:

```text
keccak256("LITHOVM_CREATE_V1" || chain_id_be_u64 || creator_word32 || effective_salt32 || code_hash32)
```

The creator is the executing factory. This is domain-separated from top-level
`LITHOVM_DEPLOY_V1`; it is not EVM CREATE2. Existing code at the derived address
causes a collision failure. No factory nonce is consumed, and failed transactions
can retry the same salt. Different template addresses with identical code hashes
produce the same child address for the same factory/salt/chain.

Initializer arguments are not in the address derivation. Different callers using
the same user salt obtain different addresses; the same caller still collides.
Caller is not transaction origin: users sharing a router share that router's
salt namespace and need an authenticated per-user policy in the router.
Deployment journals store the effective salt, not the input user salt. Existing
predictions using raw salts are incompatible and must be recalculated. Template selection
must be authenticated/pinned by the application and reviewed with chain owners.

Child code is capped at 65536 bytes. The VM charges the existing host-operation
base (40) plus statement/operand/string-copy charges. The host then charges
`100 + code_bytes` and forwards the remainder to initialization. This code charge
also applies to subsequent collision/initializer validation failures. Not enough
gas for it consumes the forwarded limit. Missing/corrupt/oversized templates are
rejected before this code charge. Every initializer/descendant gas charge is
returned to the parent; final caller gas exhaustion still rolls everything back.

The host now caps registration records at 64 per transaction (including a
top-level deployment), with atomic failure if exceeded. Successful records feed
the [native receipt handoff](LITHOSCAN_NATIVE_RECEIPTS_V1.md), not untrusted token
events. Actual persisted chain receipts and explorer ingestion remain open.

These are candidate prices, not approved chain fees. The current state interface
loads a whole template record before the size check: production code-only reads,
bounded database allocation, pre-execution parsing/hashing costs, persistence/rent
and DoS limits still require implementation and review. The code-size cap applies
to this new child operation, not retroactively to top-level deployment v1.

## Versioning and host integration

Programs containing creation emit `lithovm-native-v14`; other programs keep their
existing v11/v12/v13 encoding. New statement tag 15 encodes the four fixed operand
expressions, a big-endian u16 argument count, then type byte/expression pairs.
Older headers reject tag 15. Mixed deferred `call_contract` and synchronous
creation/invocation are rejected program-wide, including unreachable alternative
functions. v14 can mix `invoke` and `create_contract`. Existing legacy frames keep
their deferred ordering.

Use typed `deploy_values`/`execute_values`. Scalar and unhosted VM entry points
do not execute creation programs. `ExecutionHost::create` extends the existing
transactional seam; its default implementation rejects unsupported creation so
older adapters fail closed. The provided adapter uses the same `StateTransaction`
as calls and transfers, not a nested independent commit.

## Finance integration fixture

`sdk/contracts/standards/finance_factory_v14.lithic` pins a deployed template and
initializer selector during one-time initialization. Deploy it with the existing
atomic initializer interface; the host now rejects publishing it uninitialized.
Its `create` accepts salt, metadata, decimals, base-unit supply and four flags,
passes `msg.sender` as creator, increments its count and emits `TokenCreated`
after successful child initialization. It creates the existing v12 finance-token
fixture; LAX remains separate.

Tests cover all 16 flag profiles, non-18 decimals, exact metadata, user supply and
ownership, zero factory token balance, untouched initialized template state,
address/event attribution, collisions, prefunding, failure/retry, all insufficient
gas budgets of a representative creation, invalid code/signatures/arguments,
code-size limit, immediate child calls and nested deployment rollback.

This is not full OpenZeppelin differential conformance or a live finance migration.
Native signing/RPC, consensus creation receipts and LithoScan indexing are still
needed; a contract's `TokenCreated` event alone is not an authenticated code
registration receipt. Security review, longer fuzzing and Makalu failure/recovery
tests remain mandatory. No deployment or chain activation is authorized here.
