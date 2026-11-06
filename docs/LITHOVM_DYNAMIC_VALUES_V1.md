# Candidate dynamic value envelope

Implementation: `lithovm_bytecode::values::{Value, encode, decode}`.
The envelope is also used as the bounded value model for the local v12 string
execution candidate described below. It is not a chain transaction, persistent
storage encoding or RPC interface. Scalar compiler output remains v11.

The envelope is `LVAL` followed by version byte 1 and a big-endian u16 value
count. Each value starts with a one-byte tag. Tags 1 through 5 retain the
existing scalar types and their canonical 32-byte words. Tag 6 denotes a
string: big-endian u16 byte length followed by exactly that many UTF-8 bytes.
No padding, trailing bytes or unknown tags are accepted. The binary-value
extension below defines the only additional envelope version.
Strings preserve embedded NUL, combining marks and all valid UTF-8 without
normalization or truncation. Empty strings are permitted.

Candidate limits are 64 values, 4096 UTF-8 bytes per string, and 65536 bytes
per complete envelope. The aggregate limit includes headers. Encoding validates
limits before allocating output. Decoding checks total size before parsing and
checks count/string bounds before allocation. These are implementation limits
for review, not a consensus-approved schedule or Solidity compatibility claim.

Independent example: two strings `Aé` and empty encode as
`4c56414c01000206000341c3a9060000`.

Tests cover this vector, exact round trips, Unicode preservation, maximum string
length, total/count limits, malformed/truncated payloads, invalid UTF-8, trailing
data and noncanonical scalar words. The existing bytecode-decoder fuzz target
also checks canonical re-encoding of every accepted value envelope.

## Executable v12 candidate (2026-09-26)

`lithc --emit lithovm` lowers `string` parameters, return types, local bindings,
scalar state fields and event fields. A program with a string schema uses
bytecode version 12 and target `lithovm-native-v12`; scalar programs retain
their v11 bytes, selectors and gas schedule. Tag 6 is not a 32-byte word.
Older bytecode versions reject string schemas. String map keys/values, source
string literals/constants, concatenation and general arrays remain unsupported.
Metadata currently enters through parameters, not literals.

`Vm::execute_values_transactionally` accepts `values::Value` and returns
`ExecutionOutcome<Value>` with typed return/event values. Existing scalar VM
APIs reject dynamic programs. String storage starts empty, supports exact UTF-8
equality and participates in clone-and-commit rollback. Internal shared string
buffers avoid repeated allocations for locals; no string is truncated or packed
into a scalar word. The scalar host APIs reject v12 deployment, including
requests without an initializer. Explicit `deploy_values` and `execute_values`
host APIs now carry `Value` arguments/results and typed events through atomic
deployment and deferred call trees. Their event-value envelope budget is also
65536 bytes across the entire transaction, not just each frame. Child call
intents remain zero-argument and do not expose return data to the caller.

Candidate additional gas is one unit per UTF-8 byte on argument ingress,
parameter/local/storage reads, storage writes, event/return materialization and
both equality operands. Identity returns charge ingress and return bytes.
Existing instruction/statement costs still apply. Strings remain bounded to
4096 bytes, argument envelopes to 65536 bytes/64 values, and cumulative emitted
value envelopes in a v12 invocation to 65536 bytes (each event includes its
7-byte envelope header and value tags/lengths). These charges and bounds are
local candidates, not consensus-approved pricing. Persistent storage rent,
host/FFI costs and full memory/receipt pricing remain open.

Compiler/runtime tests cover Unicode, embedded NUL, empty and maximum-sized
values, byte-based limits, locals, equality without normalization, storage,
events, exact gas, version downgrade, scalar compatibility, event-output limits,
revert and every insufficient gas budget for a representative stateful call.
The source/artifact verifier rebuilds v12 too; this is not on-chain verification.

Remaining before Amir's factory can use this path: source literals and an
approved collection profile; ordered
synchronous calls and contract creation; persistent chain/gateway and RPC
integration; independent review and Makalu acceptance. No native deployment
availability or production readiness is established by these local tests.

## Binary values and runtime hashing candidate (2026-11-06)

The compiler, VM, transactional host and local FFI support bounded `bytes`
parameters, returns, locals, equality, event fields and synchronous call
arguments/results. `keccak256(data)` accepts one `bytes` operand and returns
`bytes32`. It hashes the exact binary payload, including embedded NUL and
non-UTF-8 bytes, using Keccak-256 rather than SHA3-256. Existing compile-time
`keccak256("literal")` constants keep their behavior.

These programs require bytecode **v15**, target `lithovm-native-v15`, value type
tag 7 and hash instruction opcode 20. Version selection includes nested blocks,
typed call results and unused event schemas. Downgraded headers reject the new
types/instruction. This VM format number does not imply LEP100-15 conformance.
V15 uses the hosted execution mode introduced by v13; the typed transactional
host supplies that mode, including for pure binary/hash entrypoints. Deferred
`call_contract` cannot be mixed into v15 programs. Existing v1-v14 programs
retain their encoding and execution rules.

An envelope containing a binary value uses `LVAL`, version byte 2, and the same
big-endian u16 count. Tag 7 is followed by a big-endian u16 byte length and the
exact bytes. Other values retain their existing tags and encoding. V2 without
a binary value is rejected, as is tag 7 in v1. Envelopes without binary values
continue to use v1, so every accepted envelope has a unique canonical encoding.
For example binary `[0x00, 0xff, 0x80]` followed by string `A` encodes as
`4c56414c02000207000300ff8006000141`.

Candidate bounds remain 64 values and 65,536 encoded bytes in aggregate, with
at most 4,096 bytes per binary value. The existing cumulative event budget also
counts binary payload bytes, tags and headers. Bounds are checked before
argument/return copying; out-of-gas and explicit revert discard state, events
and nested effects. Binary storage, binary map keys/values, source literals,
concatenation, indexing, general arrays and signature verification remain
unsupported and do not gain semantics from this change.
The v16 extension below adds bounded concatenation/indexing and signature
verification; these remain absent from the reviewed v14 release.

Candidate copy/equality charges follow string byte charging. Hashing adds
`30 + 6 * ceil(payload_bytes / 32)` gas plus the existing instruction charge,
before hashing; the live external fuel sink receives the same charge. This is
a local test schedule, not approved production pricing or a gas-cap decision.

Tests exercise independent empty/`abc` Keccak vectors, arbitrary binary and
maximum-sized values, canonical/truncated/malformed ABI inputs, version
downgrades, typed nested calls, event limits, persisted hash-state reload
through the C ABI, external fuel and every insufficient gas budget of a
representative stateful hash call. The `binary_values` fuzz target checks
deterministic execution/replay and transaction rollback across binary inputs,
gas budgets, rejection and revert cases.

`lithdev` can decode binary results, but rejects preparing deploy/call intents
for v15 against its documented Makalu v14 profile. No reviewed node binary,
runtime library, diagnostic pin, deployed contract or standard is replaced.
The LEP100-15 signing proposal remains subject to the maintainers' decision;
native account behavior and security/production acceptance remain separate.

## Account primitives and proposed base-account candidate (v16)

The separate development branch adds bytecode v16, `lithovm-native-v16`.
Existing v1-v15 encodings remain readable and existing sources keep their
previous target unless they use a new primitive. LVAL framing remains as above;
no new envelope tag or silent conversion is introduced.

| Source operation | Arguments | Result / rule |
| --- | --- | --- |
| `bytes_len` | `bytes` | `u64` length |
| `bytes_concat` | `bytes, bytes` | Exact concatenation, at most 4,096 bytes |
| `bytes_slice` | `bytes, u64 start, u64 length` | Checked range; empty slice at the end is valid |
| `bytes_get` | `bytes, u64 index` | Unsigned byte as `u64`; checked index |
| `word_bytes` | One scalar | Its canonical, big-endian 32-byte representation |
| `bytes_word` | `bytes` | `bytes32`; requires exactly 32 bytes |
| `word_address`, `word_u64`, `word_u256` | `bytes32` | Checked conversion; high bytes cannot be truncated |
| `secp256k1_verify` | `bytes32 digest, bytes signature, address signer` | Strict public-key recovery/verification, returning `bool` |
| `contract.address` | Authenticated host context | Executing account, distinct from `msg.sender` |

The new instruction opcodes are 21 through 31 in table order (the three checked
word conversions occupy 27-29). Every new instruction is gated on v16. A
program selecting v16 uses hosted execution. Intrinsic names remain valid
binding names when used without call parentheses. Host arguments may contain
nested intrinsic calls without splitting their internal commas.

Signature verification accepts only 65 bytes `r || s || v`, recovery value 27
or 28, valid nonzero curve scalars, low S and the expected nonzero address.
It verifies the supplied digest without hashing it again or normalizing a
noncanonical signature. Invalid signatures return false. The runtime pins
`k256 0.14.0`; the selected normal dependency graph uses `crypto-bigint 0.7.10`,
within the patched range for [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html).
The older development-only graph remains disclosed in the completion record.
The 6 November OSV query found no entries for the 53 selected normal runtime
packages; this scoped lookup is not a security audit or release acceptance.

Byte copies charge their bounded output length before allocation, in addition
to existing instruction/operand charges. Verification charges a local candidate
3,000 gas before crypto, including invalid signatures. All charges reach the
same live external fuel sink. These test prices do not establish production
gas economics.

### Native payload and read-only calls

```lithic
let result: bytes = invoke_payload(target, value, payload);
let checked: bytes = invoke_static_payload(verifier, 0, payload);
```

Payload is the full 32-byte native selector followed by one canonical LVAL
argument envelope. Results are a canonical LVAL envelope containing the typed
return value. Source bindings must be immutable `bytes`; calls occupy statement
opcodes 16 and 17, gated on bytecode v16. Malformed envelopes, unknown selectors,
wrong argument types, unsupported targets and failed calls fail the enclosing
transaction. The encoded return envelope itself must fit the 4,096-byte binary
value limit; larger returns fail atomically rather than being truncated.

Read-only mode is set by the caller and inherited by every descendant. Native
writes, map writes, events, transfers, creation and deferred legacy calls are
rejected. Typed or payload calls cannot turn off the inherited mode or send
value. Successful views do not persist implicit zero/empty storage defaults,
including through the direct VM interface. Host state and balances are not
committed by read-only frames. Existing depth and reentrancy guards still apply.
These are native calls; no EVM calldata adapter or EIP-1271 support is added.

### Executable account engineering fixture

[`lep10015_native_account_candidate.lithic`](../sdk/contracts/standards/lep10015_native_account_candidate.lithic)
is generated deterministically by
[`generate_native_account_candidate.py`](../scripts/generate_native_account_candidate.py).
It compiles and executes the following locally:

- Canonical transaction/signature ABI arrays, bounded to 16 calls/signers and
  4,096 bytes per transaction or signature bundle; general `Vec`/struct syntax
  remains unsupported. Aggregate byte bounds can be stricter than element counts.
- Key and native contract signers; strict signatures, unique credentials,
  sorted unique signer IDs, current threshold and fresh policy-epoch binding.
- Domain/account/chain binding, signed metadata, validity windows, checked u256
  nonces, direct signed execution, reserved proposals and out-of-order execution.
- On-chain approvals and revocation tombstones. Explicit reapproval may count
  again without restoring a revoked off-chain signature.
- Atomic native transfers and native contract/token calls, source-order events,
  rollback on failure/OOG and recovery of an already reserved proposal.
- Threshold changes, signer addition/removal/replacement and cancellation only
  through threshold-authorized self-dispatch. Public administrative methods
  reject external owners. Signer IDs are never reused; configuration changes
  increment the authorization epoch and invalidate stale-policy proposals.
- Native contract signature verification through transitive read-only calls,
  requiring exact canonical bool true, with no verifier state/log/value effects.

The initializer consumes a canonical ABI `(address,uint16)[]` descriptor array
inside its native `bytes` argument. Administrative native selectors use the
fixture's declared u64 operands and strict limits. They are not a certified
draft-u32 interface or the existing Litho Vault EVM interface.

The exact proposed signing vectors remain unchanged at SHA-256
`8fbc504280a07902d1b9bdc1517061c35dee1f708e8b938f879dcf5d31fc8c45`.
Both digest vectors, their key signatures and derived signer IDs pass through
the compiler/runtime. This proves behavior against proposed fixtures, not a
maintainer decision, official conformance or a chain deployment. The proposal
in [official PR #14](https://github.com/KaJLabs/Lithic/pull/14) still needs its
hash-bound maintainer decision before interoperability can be claimed.

This fixture rejects STATIC_CALL transaction operations, delegate calls,
EVM/EIP-1271 adapters and unassigned/native-PQ algorithms. It does not implement
guardian recovery, agents/AI policies or a detached nested-account signature
format. Their required status and exact profile must follow the authoritative
standards decision; they are not silently treated as implemented. The native
FFI chain harness remains nonpayable, so in-memory native-value tests do not
establish native custody on the chain. `lithdev` continues to reject v16 intents
for the documented Makalu v14 profile.

### Code admission and deployment budget

The current account compiles to **23,924 bytecode bytes**, below the 65,536-byte
code-admission bound. This is not a deployability or affordability guarantee.
The current FFI contract record serializes bytecode as hex, requiring at least
47,850 value bytes for that field alone. Under the unchanged disabled-candidate
growth formula (20,000 gas per additional rounded 32-byte allocation unit),
code alone therefore needs at least **29,920,000 growth gas**, exceeding the
current 10M native test envelope before write, key, VM or other state charges.
This is an offline lower bound, not a keeper benchmark result. Final pricing,
cap/layout policy and chain compatibility remain separate required gates;
local tests do not clear this known admission-versus-deployability gap.
