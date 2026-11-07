# LEP100-15 implementation decisions

Status: required before interoperable signing or a conformance claim

The LEP100-15 draft defines the fields included in its domain and transaction
hash but does not yet define their canonical bytes. Implementations must not
choose these details independently because signatures, wallets and contracts
would become incompatible.

Freeze the following normative values and publish fixed positive and negative
test vectors:

1. Standard version value and byte representation.
2. Hash algorithm for the domain, call list and final transaction digest.
3. Integer widths and byte order for chain ID, nonce, value and timestamps.
4. Address representation and validation rules.
5. Byte-array and list length-prefix encoding.
6. Canonical encoding of each call: target, value, data and operation.
7. Operation numeric values and whether `STATIC_CALL` may carry value.
8. Meaning of zero `valid_after` and `valid_until` values.
9. Whether `metadata_hash` is authorization-critical in every profile.
10. Signature envelope fields, algorithm identifiers and signer identity
    derivation for secp256k1, contract and post-quantum signers.
11. Canonical secp256k1 recovery value and low-S rules.
12. Exact nonce consumption point for proposals and direct signed execution.

Required vectors:

- one single-call transaction and one multi-call transaction;
- the encoded domain, encoded call list, domain hash and final digest;
- valid signatures for every required signer algorithm;
- wrong chain, wrong account, wrong nonce and modified-call failures;
- duplicate signer and non-canonical signature failures;
- zero and boundary timestamp cases.

Until these values are frozen, the compiler and VM may implement language and
execution primitives, but no artifact should claim LEP100-15 signing
interoperability or production conformance.

## Proposed engineering candidate, 6 November

[Official PR #14](https://github.com/KaJLabs/Lithic/pull/14) contains the single
proposed signing profile and fixed vectors. Their hash-bound maintainer decision
remains required. This branch adds native v16 byte/signature/read-only primitives
and an executable, explicitly unapproved base-account fixture using that
proposal's choices. Its canonical transaction hashes, signatures and signer IDs
match the proposed independent fixtures; this does not convert the proposal
into a normative standard.

See the [supported behavior, implementation bounds and deployment gap](LITHOVM_DYNAMIC_VALUES_V1.md#account-primitives-and-proposed-base-account-candidate-v16).
General `Vec`/struct syntax, guardian recovery and detached nested-account
signatures are not implemented. EVM/EIP-1271, STATIC_CALL transactions and native
PQ schemes remain rejected. Final account/profile scope, interoperability,
official conformance and independent security acceptance remain open.
