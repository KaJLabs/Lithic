# Module Specifications

## Standard modules

### LEP100 Fungible Token
Responsibilities:
- balances
- transfers
- approvals
- mint/burn extensions
- metadata references
- capability-gated administration

### LEP100 NFT
Responsibilities:
- ownership
- approvals
- safe transfers
- metadata
- royalty hooks

### AI Provider Registry
Responsibilities:
- provider registration
- model hash registration
- SLA and pricing metadata
- active/inactive provider status

### Budget Escrow
Responsibilities:
- reserve budget
- ceiling enforcement
- settle from valid receipt
- refund on timeout

### Provenance Verifier
Responsibilities:
- canonical receipt digest
- signer validation
- replay protection
- model hash binding

### ZK Verifier Registry
Responsibilities:
- verifier key storage
- circuit/version binding
- proof policy and validation adapters

### Bridge Mint/Burn
Responsibilities:
- source event binding
- replay protection
- mint authorization
- burn/escrow lifecycle

### PPAL Registry
Responsibilities:
- privacy-preserving account linking
- nullifiers
- scoped commitments
- revocation

## Tool modules

### lithc
Compiler frontend/midend/backend entrypoint.

### lithfmt
Stable formatting rules for `.lithic` code.

### lithlint
Static checks for style, determinism, security, and upgrade safety.

### lithdev
Local chain, deployment helper, test harness launcher.
