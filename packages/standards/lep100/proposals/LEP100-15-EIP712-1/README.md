# LEP100-15-EIP712-1: maintainer decision requested

Status: **proposed; not accepted, implemented or certified**.

This submission follows this repository's contribution/PR process. It requests
an explicit acceptance or amendment from the KaJ Labs LEP100 maintainers, with
the standard's named author J. King Kasr as appropriate. It does not presume a
particular GitHub account or that a developer signature establishes maintainer
authority. The canonical [draft](../../LEP100-15.md) is unchanged.

The two files preserve the exact previously reviewed proposal bytes:

| Artifact | SHA-256 |
| --- | --- |
| [SIGNING_PROFILE.md](SIGNING_PROFILE.md) | `93d3c635d55a011b52f75c91923e310cbd5dc4d99afa910d3206c6e07859a99c` |
| [SIGNING_VECTORS.json](SIGNING_VECTORS.json) | `8fbc504280a07902d1b9bdc1517061c35dee1f708e8b938f879dcf5d31fc8c45` |

The profile proposes one answer to all twelve open signing choices, exact
EIP-712 type strings/preimages, strict secp256k1 encoding and nonce/authorization
semantics. Zero expiry and policy-epoch binding require explicit normative
clarification. Existing Litho Vault EVM approvals sign a different structure;
no byte compatibility or signature conversion is claimed.

The two fixed fixtures include preimages, canonical transport and public-test-key
signatures. The supplied local proposal check had 31 passing signing/encoding
tests, using manual ABI preimages checked against ethers 6.15.0's typed encoder.
Those checks are engineering evidence, not an independent crypto audit or
native-account conformance result. This PR adds no runtime or account code.

## Requested decision

Please record a maintainer decision here bound to both artifact hashes:

```text
Decision: ACCEPTED / AMENDMENT REQUIRED
Standard and profile: LEP100-15 / LEP100-15-EIP712-1
Signing profile SHA-256: 93d3c635d55a011b52f75c91923e310cbd5dc4d99afa910d3206c6e07859a99c
Signing vectors SHA-256: 8fbc504280a07902d1b9bdc1517061c35dee1f708e8b938f879dcf5d31fc8c45
Accepted scope or exact required amendments:
Maintainer authority/reference:
```

An amendment creates a new version and new hashes; it must not silently edit an
accepted artifact. Silence, a successful CI job, a submitted PR or a general
acknowledgment is not this decision. Native conformance, security acceptance,
gas policy and production rollout remain separate.

## Source-reference map

`SIGNING_PROFILE.md` is preserved byte-for-byte, including its original workspace
citations. Its core sources can be read without that workspace:

- [Open signing decision register](../../../../../docs/LEP100_15_IMPLEMENTATION_DECISIONS.md).
- [LEP100-15 draft](../../LEP100-15.md) and the official EIP/ABI links in the profile.
- The inspected compiler/runtime source is pinned at
  [`f7c259ae0a8e5d9e5fb6e7b0b7cbac1428fac8ce`](https://github.com/KaJLabs/Lithic/tree/f7c259ae0a8e5d9e5fb6e7b0b7cbac1428fac8ce/packages).
- The supplied client draft is identical after BOM/newline normalization, as
  disclosed in the profile. The optional existing Vault SDK/fixture observations
  are bounded supplied-source evidence with their hashes; no private client
  implementation or credentials are published in this submission.

The existing Vault source and SDK observations motivate compatibility cautions;
they do not establish the proposed standard's normative rules. Maintainers may
accept or amend the proposed rules independently of that implementation.
