# Bank-backed native custody candidate

Engineering extension, 7 November 2026. The existing first gateway profile stays
nonpayable and uses request version 1 / `lithovm_execute_v2`. This extension is
separately selected with **both** `lithovm_chain_lab,lithovm_bank_candidate` build
tags and a matching chain/FFI source pair. It is not an activation mechanism or
an approved live runtime profile.

The matching chain commit is
`315ab5da4a06ad6e8db2e06910ddc0449f3d4940`, based on
`3cadea9d252be3a4aec63f5e1ba200560eb14007`. Its exact six-file patch and
binding metadata are included in `integration/native-chain/chain-native-custody.patch`
and `custody-candidate.json`. The patched SDK source is unchanged from the
frozen v17 review snapshot. Do not substitute a different chain/FFI combination.

## Ledger and atomicity

Native accounts are ordinary addresses in the EVM-denominated SDK bank ledger.
Fund an account address using an ordinary native bank/EVM transfer; do not send
value to the gateway. Prefunding survives native deployment. Nonce and EVM code
metadata remain intact. `transfer_native` and native call values stage balance
changes in the same transaction as contract records, per-key maps and events.
Native transfers do not execute an EVM recipient fallback. This extension adds
no native-to-EVM call or ERC-20/ERC-1271 interoperability.

The new `lithovm_execute_v3` ABI requires request version **2**, a state reader,
an independently supplied bank reader and a live fuel callback. Caller JSON
cannot supply balances or transaction value. The bank callback receives exactly
20 address bytes and writes one big-endian uint256 into a 32-byte output; return
1 for success and 0 for failure. Pointers are synchronous and must not be retained.
Callbacks must not unwind or reenter the invocation. v1/v2 exports and their
request/response shapes remain unchanged.

The response adds `bankVersion: 1` and `balances`, a sorted array of changed
`{address,before,after}` entries. Addresses are canonical lowercase 20-byte hex;
amounts are canonical lowercase 32-byte hex. There are at most 128 distinct bank
accounts per native transaction. The FFI caches original independent reads,
checks conservation with a 264-bit sum, and never writes external bank state.
Failures return no balance, KV, deployment or event effects.

The Go adapter independently validates the entire batch, original reads,
conservation and current frame balances. It applies changes through the actual
Evmos StateDB journal. Pending ordinary EVM transfers are visible. Native KV,
logs, account creation and bank balances all revert on a settlement failure or
an enclosing EVM revert. Only the ordinary successful EVM commit reaches the
SDK account/bank keepers. Module, blocked and incompatible auth accounts are
rejected; the extension grants no independent mint or burn authority. The local
signed-message test verifies unchanged SDK supply through the actual keepers.

## Gas and receipts

The existing shared 10M native envelope remains unchanged. Actual SDK bank/auth
reads use the same live frame budget, with the pinned SDK KV gas configuration
explicitly restored for these reads. EVM ante/query contexts otherwise disable
that pricing; inheriting their zero rates would understate estimates. The
extension adds provisional bookkeeping prices of 9,000 per changed address and
25,000 per newly created EVM account, plus actual serialized bank-log pricing.
These are disabled-test candidate rates, not approved production economics.
Existing native record/map/log prices also remain provisional where previously
documented. Final bank-write/account-growth policy requires Foundation review.

The existing `LithoNativeLab(bytes)` receipt stays unchanged. A separate
`LithoNativeBalancesV1(bytes)` topic contains
`{"version":1,"changes":[{"address":"0x…","before":"0x…","after":"0x…"}]}`.
This is a **net transaction settlement** record, not a per-transfer execution
trace. Charge its actual emitted bytes before effects. Unknown topics remain
ignored by the existing deployment decoder. Neither log alone proves canonical
inclusion, finality or physical storage durability.

Native-envelope exhaustion remains the ratified non-REVERT EVM failure that
burns the affected call frame's remaining gas, including gas above the native
cap. Wallets and integrators should avoid excessive gas limits or forwarding.
Reverts roll back effects; gas consumed by failed execution is not refunded.

## Verification

Use Rust 1.96.0 and the pinned Go 1.22.12 chain toolchain, the matching chain
checkout and the disclosed patched SDK. Build `lithc` and `lithovm-ffi` together.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p lithc -p lithovm-ffi
cd integration/native-chain
go test -tags lithovm_chain_lab .
go test -tags lithovm_chain_lab,lithovm_bank_candidate .
# In the matching chain checkout:
go test -tags lithovm_chain_lab,lithovm_bank_candidate ./x/evm/keeper \
  -run 'TestKeeperTestSuite/(TestNativeCustody|TestLithoVMCandidate)'
```

The chain fixture `x/evm/keeper/testdata/native_custody.json` is the actual output
of `lithc --emit lithovm` for its adjacent source, identical to the integration
Vault source. Rebuild and compare that artifact when the compiler changes.
The `native_custody` fuzz target exercises the exported v3 ABI and independently
checks expected before/after balances and failed-call isolation. Release builds
use the new `native-account-v17-bank-candidate-v1` smoke profile, exercising the
packaged v3 library as well as the unchanged v2/account/signature smoke. Old
release smoke profiles remain supported explicitly.

Coverage includes quorum-based proposed LEP100-15 custody, replay rejection,
failed multi-call rollback, unauthorized spending, insufficient balance,
recipient overflow, SDK policy failure after partial settlement, live OOG,
ancestor revert, prefunding, metadata retention, simulation discard, gas
estimation replay and exact supply conservation. These are local synthetic
tests, not Makalu end-to-end or production acceptance.

## Remaining release gates

The LEP100-15 signing profile and fixed vectors still need the maintainers'
explicit hash-bound decision. This extension and its exact compiler/chain pins
need independent security review and final source acceptance. Final custody,
gas/cap/growth policy, native-store lifecycle/upgrade and ulitho/Makalu
compatibility, approved validator workload evidence, signed release identity,
and exact network installation/deployment acceptance remain separate gates.
The earlier failed full 50M benchmark and accepted bounded diagnostics remain
unchanged; this work authorizes no further benchmark or retry.

No LAX, MultX, live contract, live key, custody asset or public gateway is used
or activated by these tests. The existing frontend token-deployment and
LithoScan verification adapters remain disabled pending an approved matching
network runtime profile. `lithdev` continues to reject preparing v17 for its
documented Makalu v14 profile rather than imply compatibility.
