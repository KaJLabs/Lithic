# Litho Finance token factory acceptance case

Inspected 2026-09-26. Client package:
`client-work/litho-finance-lithic-integration-case.zip` (in the infrastructure workspace).
SHA-256: `28aadfb91d749fec0ce67b98ad5f88c94ddb25f002d3acbba215733ec8272bac`.

The six-file package contains two Solidity contracts, two frontend TypeScript
files, ten Hardhat test cases and a README. It provides a representative
integration case, not a standalone buildable frontend: package manifests,
lockfiles, Hardhat configuration, generated types and frontend imports are absent.
No client tests were run during this source inspection.

The README lists EVM factory addresses for chains 9005 and 700777. These are
client-supplied references; inspection of this archive does not verify live
code, deployment provenance or native LithoVM availability.

## Required behavior and current gaps

| Client behavior | Current native candidate | Required acceptance |
| --- | --- | --- |
| Balances and allowances | Typed single/nested maps exist | Port ERC-20 behavior, including allowance edge cases, against the pinned OpenZeppelin 5.0.2 reference |
| Dynamic name and symbol | v12 executable UTF-8 ABI/storage/events with byte gas; v14 factory passes exact metadata into fresh tokens | Chain encoding, wallet/explorer display and broader conformance; never silently truncate client strings |
| Configurable decimals and supply | `u64`/`u256` values exist | Preserve `uint8` decimals range and exact base-unit supply; test non-18 decimals |
| Factory creates a child token | v14 code-template creation with salted identity, bounded code/gas and atomic initialization; all 16 finance profiles tested | Approved salt/address/gas policy, persistent creation receipts, chain integration and Makalu acceptance |
| Supply and ownership assigned to original caller | Caller context exists | Pass creator explicitly to child initializer; factory must not receive supply or ownership |
| Mint/burn/pause/ownership flags | Separate finance_token_v11 fixture implements the four flags, authorization, delegated burn and ownership changes; 16 combinations tested | Full metadata/factory integration and OpenZeppelin differential conformance; LAX remains separate |
| TokenCreated receipt | Native typed events exist | Address plus string metadata, indexed event policy and committed receipt decoding |
| Wallet submission and confirmation | No native chain adapter | Authenticated transaction, simulation, real hash, inclusion/failure and explorer status |

This factory only creates a new token. It does not pull existing ERC-20s.
Native-to-EVM `transferFrom` interoperability is a separate later acceptance
case for the other factories. Local v14 creation now provides ordered child
initialization and the new address to the parent; this is not a chain deployment.

The LAX fixture is not a drop-in replacement: it has fixed metadata and supply,
and does not implement configurable minting, pausing or ownership. Its approved
distribution and deployment gates remain independent of this generic factory.

## Frontend findings

`TokenCreationService.ts` currently simulates a Lithic deployment with a delay,
random hash and random address, and invokes `onSubmitted`. Replace that path
with an explicit unavailable error until a real native adapter is available.
The production UI must not present a generated identifier as a submitted transaction.
Whether this supplied branch is active in the live UI has not been verified.

The supplied Solidity receipt path also needs explicit success-status checking,
event-emitter matching against the factory, and rejection of a missing or
ambiguous TokenCreated event. Checking creator and submitted metadata further
binds the selected event to the user's request. Preserve this EVM workflow
while introducing an independently tested native adapter.

## Test plan

Retain all ten supplied tests as the initial behavioral baseline. Add tests for
factory initialization failure and retry, no partial child/storage/event commit,
creator-versus-factory identity, all feature combinations, burnFrom allowance
handling, paused mint/burn/transfer behavior, ownership changes, overflow,
out-of-gas, malformed receipts and duplicate/wrong-emitter events.

The replacement service and six passing boundary regression tests are now in
`sdk/integrations/litho-finance/`. Native requests reject before submission;
the existing Solidity path validates receipt success, factory emitter, event
cardinality, metadata and token address. Tests execute the actual service with
mocked external dependencies; this is not a live frontend integration.

Follow-up, 2026-09-26: local v12 compiler/VM/host metadata support now exists.
Full frontend access is available at `amirmughal22/Litho-Finance`; its local
safety branch commit `2822474` passes 24 tests, typecheck, lint and production
build. Native requests remain disabled. These tests use the real ABI decoder
but mocked wallet/RPC boundaries; no live application was changed.

The local frontend follow-up through `a38191c` also passes four isolated browser
tests with a synthetic rejecting wallet; no real transaction is signed.
The [v13 native-call candidate](LITHOVM_SYNCHRONOUS_V13.md) now provides ordered
calls with typed arguments/results. A host test pulls tokens from the existing
v12 token fixture using the caller contract's allowance and proves token-state
rollback when that caller subsequently rejects. This is not EVM interoperability
or a complete factory implementation.

The [v14 factory candidate](LITHOVM_CREATION_V14.md) now creates a fresh token
from a pinned deployed template, with metadata and creator identity passed to
its initializer. All 16 profiles, non-18 decimals, balances/ownership, untouched
template storage, failure rollback and collisions are covered locally.

Next native implementation steps are consensus deployment/creation receipts,
chain/gateway integration, real wallet/signing tests and Makalu acceptance.
Public-factory salt handling and creation pricing need policy/security review.
No additional workflow description is needed from Amir for this first case.
