# LithoVM deployment interface v1

Status: deterministic library interface and in-memory conformance tests; no
chain RPC or deployment authorization

## Request

`DeployRequest` contains the authenticated deployer, account nonce, chain ID,
native v11 bytecode, optional initializer, attached native value, gas limit
and block context. The embedding chain must
source deployer, nonce and block values from consensus state; clients must not
be able to assert them independently.

The host parses the validated bytecode and registers every function under the
full 32-byte `keccak256(name(type,...))` selector. Registrations cannot be
supplied or overridden by a deployer. The compiler emits the same canonical
signature and selector in its artifact.

The bytecode code hash is:

```text
keccak256(bytecode)
```

The canonical 20-byte contract address is the low 20 bytes of:

```text
keccak256(
  "LITHOVM_DEPLOY_V1" ||
  chain_id_be_u64 ||
  deployer_word32 ||
  nonce_be_u64 ||
  code_hash
)
```

It is returned as the canonical 32-byte address word with twelve leading zero
bytes. The chain adapter must bind `nonce` to its authenticated account
sequence and reject replay before calling this interface.

## Atomicity

Deployment validates bytecode and derives entrypoints before opening a state
transaction. Within one transaction it rejects address collisions,
debits attached value, stores bytecode and code hash, and runs the optional
initializer. Code, balances, initializer storage and events commit together.
Invalid bytecode, selector collisions, insufficient balance, initializer
failure, out-of-gas or commit failure publishes none of them.

The initializer sees the deployer as `msg.sender` and attached value as
`msg.value`. This closes the LAX candidate's first-caller initializer race once
the chain routes deployment exclusively through this atomic interface.

Every contract load verifies persisted bytecode against its stored code hash.
Mismatch fails closed as a state-integrity error.

## Receipt and explorer handoff

`DeploySuccess` provides contract address, code hash, optional initializer
result, total gas and committed events. This is the minimum included-state
payload for a future LithoScan deployment record and source-verification job.
The external lifecycle still needs a versioned RPC/indexer schema for
`prepared`, `submitted`, `included`, `failed`, `verified` and
`verification_failed` states. The portable schema is
`sdk/schemas/lithovm.deployment-status.schema.json`; chain RPC and indexer
implementations remain external integration work.

## Remaining gates

Contract-level creation is now available in the separate local
[v14 candidate](LITHOVM_CREATION_V14.md). Its code-template/salt identity,
mandatory boolean initializer and child code-size/gas rules do not change this
top-level v1 interface. Both paths share the outer transaction; neither provides
authenticated chain submission or deployment approval.

- consensus state/bank adapter and authenticated nonce handling;
- deploy/call RPC, signing and fee simulation;
- code-size/deployment gas pricing and denial-of-service limits;
- code/address compatibility and upgrade policy;
- Makalu success, collision, replay, initializer failure and restart tests;
- deterministic LithoScan rebuild worker and persisted indexer integration;

This interface does not authorize a Makalu, production or LAX deployment.
