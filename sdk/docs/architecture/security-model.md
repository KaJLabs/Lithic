# Security Model

## Core principles

- Capability-based access control
- Deterministic execution only
- Budget reserve before asynchronous AI execution
- Signed provenance receipts for offchain compute
- Replay protection for bridge and receipt flows
- Explicit storage schema versioning
- Upgrade safety checks for contract migrations

## Compile-time checks

- missing capability requirements
- unsafe storage layout migrations
- unchecked external receipt flows
- potentially nondeterministic iteration
- missing budget enforcement in metered flows
- bridge nonce/replay omissions
- possible reentrancy around hooks and callbacks

## Runtime checks

- capability validation
- budget reservation and ceiling enforcement
- signature verification with domain separation
- receipt replay protection
- nullifier uniqueness for privacy links
- proof verification key/version validation

## Threat classes

- unauthorized minting or admin takeover
- budget bypass in AI metered services
- forged provider receipts
- replayed bridge messages
- storage corruption during upgrades
- privacy link double-spend / nullifier reuse
