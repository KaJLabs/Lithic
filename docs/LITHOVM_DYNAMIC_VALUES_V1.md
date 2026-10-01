# Candidate dynamic value envelope

Implementation: `lithovm_bytecode::values::{Value, encode, decode}`.
The envelope is also used as the bounded value model for the local v12 string
execution candidate described below. It is not a chain transaction, persistent
storage encoding or RPC interface. Scalar compiler output remains v11.

The envelope is `LVAL` followed by version byte 1 and a big-endian u16 value
count. Each value starts with a one-byte tag. Tags 1 through 5 retain the
existing scalar types and their canonical 32-byte words. Tag 6 denotes a
string: big-endian u16 byte length followed by exactly that many UTF-8 bytes.
No padding, trailing bytes, unknown tags or alternative versions are accepted.
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
