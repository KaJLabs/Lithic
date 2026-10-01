# Native LithoVM artifact and call ABI v10

Status: implementation candidate; explicit transactional failure semantics

Target identifier: `lithovm-native-v10`

Version 10 adds two statements:

```lithic
require(condition);
revert();
```

`require` accepts exactly one `bool` expression. The runtime charges the
statement and expression before evaluating the condition. `false` fails the
call; `true` continues. `revert()` is terminal, accepts no arguments and is
charged as one statement before failing the call.

Every failure discards the execution's staged storage, events, native
transfers and contract-call intents. A later call starts from the last
successfully committed storage state. The host must apply a successful
`ExecutionResult` atomically; it must never apply effects from an error path.

Versions 1 through 9 retain their encodings. Version 9 remains the first
version with gas-bounded repeat loops. General `while` loops, unbounded
iteration, recursion, collection storage, synchronous contract execution and
consensus application of staged effects remain unsupported.
