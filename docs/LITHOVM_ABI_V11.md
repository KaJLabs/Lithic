# Native LithoVM artifact and call ABI v11

Status: implementation candidate; typed map storage and LEP100 amount
primitives

Target identifier: `lithovm-native-v11`

Version 11 retains versions 1 through 10 and inserts a map-schema table after
the scalar-storage table. Every map declares an ordered non-empty list of
scalar key types and one scalar value type. Nested source maps flatten into
one multi-key schema; for example:

```lithic
allowances: map<address, map<address, u256>>;
```

is encoded as two ordered `address` keys and a `u256` value. Keys use their
canonical 32-byte ABI words. Missing entries read as the canonical zero value.
Map writes are staged in the same `Storage` snapshot as scalar writes, so any
trap, out-of-gas result, `require(false)` or `revert()` discards all changes.

Version 11 also adds checked `u256` addition and subtraction plus `u256 >=`
for balance, allowance and authorization guards. Arithmetic overflow and
underflow trap and roll back. Decimal literals paired with a `u256` operand
are widened only when they fit the canonical 256-bit word.

The candidate gas schedule charges 25 gas for a map read and 100 gas for a map
write, in addition to statement and other expression costs. These values are
deterministic implementation constants, not yet a consensus-approved Makalu
schedule.

The executable LAX candidate at
`sdk/contracts/standards/lax_lep100_v11.lithic` exercises metadata getters,
one-time initialization, the exact initial supply, balances, nested
allowances, transfers, approvals, delegated transfers, burn, events and
rollback. It is a conformance fixture, not deployment authorization.

Deliberate limits remain: no constructor/deploy transaction, string ABI,
storage migration, ordered synchronous call effects, calldata/return data for
nested calls, consensus persistence adapter, RPC or Makalu deployment. The LAX
initial holder and approved LITHO-matching distribution are deployment-plan
inputs and are intentionally not embedded or executed here.
