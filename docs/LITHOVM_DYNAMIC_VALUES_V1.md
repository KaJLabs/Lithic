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
