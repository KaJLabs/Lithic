# Compiler + Runtime Overview

## Pipeline

```text
.lithic source
  -> lexer
  -> parser
  -> AST validation
  -> name resolution
  -> type checking
  -> capability/effect analysis
  -> determinism validation
  -> HIR
  -> MIR
  -> optimization
  -> LithoVM IR
  -> bytecode/object bundle
  -> ABI + manifest + storage schema + event schema
```

## Frontend

- Lexer: tokenization of `.lithic` source
- Parser: contract/module declarations, storage blocks, event declarations, capabilities, traits/interfaces
- AST validator: basic well-formedness and syntactic constraints
- Resolver: imports, package references, namespacing
- Type checker: primitive, collection, storage, and contract interface typing
- Capability/effect checker: verifies declared privileged flows
- Determinism validator: blocks unsupported nondeterministic constructs

## Midend

- HIR: normalized semantic form
- MIR: lower-level control-flow-friendly representation
- Dataflow analysis
- Constant folding
- Dead code elimination
- Effect/cost annotations
- Formal verification hint emission

## Backend

Artifacts emitted per package:

- `contract.lvmbc`
- `contract.abi.json`
- `contract.manifest.json`
- `contract.storage.json`
- `contract.events.json`
- `contract.receipts.json` (optional)
- `contract.proofs.json` (optional)

## Runtime services

- storage read/write
- event emission
- signature verification
- Merkle verification
- zk-proof verification
- native asset transfer
- contract calls
- AI request dispatch
- receipt settlement
- budget reserve/settle/refund
- bridge proof validation
- privacy link verification

## Determinism rules

Consensus execution must not allow:

- floating point
- wall-clock reads
- external network access from contract code
- non-canonical serialization
- unbounded loops without gas/metering
- unordered iteration where state commitment could diverge

## AI execution model

1. Contract submits request to provider registry or provider endpoint reference
2. Budget is reserved before dispatch
3. Provider fulfills with signed receipt
4. Provenance verifier checks receipt digest and signer domain
5. Budget escrow settles or refunds
