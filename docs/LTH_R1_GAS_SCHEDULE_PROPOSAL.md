# LTH-R1 storage/log gas candidate and benchmark evidence

2026-09-27. **Disabled local candidate, not approved consensus pricing.**
Lithosphere Foundation is the chain owner. Alex approved caller-bound salts and
atomic initialization and nominated **@Amir Dev as technical approver**, as relayed
by the user. That nomination does not approve these numerical rates. LTH-03 remains open pending
Foundation technical review, representative validator tests and independent retest.
No registration, activation, production contract or LAX deployment is authorized.

Review update: Amir accepts KV and log rates for the disabled candidate only;
growth pricing and the 10M cap remain deferred final policy, permitted for tests.
The earlier `max_gas=-1` RPC observation was mainnet, not Makalu. A later
[active-node Makalu capture](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md)
measured 100M at height 16,237,322. See the
[recorded review and ancestry](AMIR_GAS_REVIEW_RESPONSE.md) for the mainnet
observation and scope. Historical benchmark results below are unchanged and do
not establish production affordability.

## Proposed schedule

`integration/native-chain/gas_schedule.go` uses fixed, overflow-safe constants.
Tests compare them with the pinned dependencies to flag upstream drift rather
than silently inheriting a new gas schedule.

| Work | Candidate charge | Basis |
| --- | --- | --- |
| Every database Get | 1,000 + 3 x (key bytes + value bytes) | Pinned SDK KVGasConfig |
| Changed record Set | 2,000 + 30 x (key bytes + new value bytes) | Pinned SDK KVGasConfig |
| Persistent growth | 20,000 x positive increase in rounded-up 32-byte key/value allocation units | Proposed conservative allocation floor using EVM SSTORE-set rate |
| Actual envelope log | 750 + 8 x emitted data bytes | EVM LOG base + one topic + data |
| Unchanged record write / absent log | Zero | Neither is persisted/emitted |

Existing native VM instruction charges and the gateway's `500 + 4 x ABI-input-bytes`
precharge remain additional. Storage shrinking gives no refund. Each physical
Get is charged: an existing record's two-phase FFI size/data callbacks perform
two Gets. Missing records perform one. Growth is per changed key, not netted
against shrinkage elsewhere. Changing a same-sized record pays Set but no growth.
Byte counts include the actual serialized record and key, not logical u256 size.
The growth floor is an economic policy proposal, not measured disk cost and not
a claim of semantic or performance equivalence to EVM storage.

Reads pay lookup/key cost before Get, then value bytes before returning across
FFI (and before Rust allocates/deserializes that value). Go cannot know length
before Get, so database allocation itself remains bounded by record limits, not
prepaid byte-for-byte. Writes are sorted and compared with original bytes;
unchanged whole-record writebacks are omitted. All changed writes and the actual
log are charged before any external state mutation. Out-of-gas exposes no batch
or logs; enclosing EVM snapshot rollback remains intact.

The native frame has an aggregate 10M budget for VM + reads + writes + logs,
capped from available EVM gas. High EVM limits still preserve unused gas on
successful work. The Rust interpreter receives its initial VM limit: Go charges
callbacks live but reconciles instruction gas after FFI returns. This is **not**
a single live cross-language instruction meter; failed calls can do bounded VM
work before that final reconciliation. A shared fuel interface remains required
for a stronger production CPU-work guarantee. No claim of full LTH-03 closure.

## Reproduction and local tests

Before the change, `TestNewStatePaysKVAndGrowthFloor` failed with
`charged=1102 minimum-write-and-growth=546600` for the Counter fixture. After the
change it passes. Other checks cover:

- exact deployment budget succeeds; one gas less and no lookup budget fail
  with no persisted records/logs;
- getters pay physical reads but no unchanged Set or empty log;
- an unchanged setter emitting an event pays the exact envelope LOG1 formula;
- allocation-unit boundaries, no shrink refund, overflow saturation;
- canonical native read/write namespaces (partial LTH-09 hardening);
- existing rollback/recovery, high-frame-gas and keeper EstimateGas regressions.

Full tagged native-chain tests pass with both debug and optimized Rust libraries.
The full tagged Evmos keeper suite passes with the debug library (13.664s)
and optimized library (8.048s). The untagged Go/FFI suite also passes.
This is keeper invocation, not HTTP RPC, ante-handler reproduction or Makalu.

## Optimized local benchmark

Raw output: [release benchmark](evidence/LTH_R1_GAS_RELEASE_BENCHMARK_2026_09_27.txt).
Linux WSL2 6.6.87.2, x86_64 Intel i7-8850H, Go 1.22.12, Rust 1.96.0 release FFI.
Single Go CPU, 100 iterations x 3 repetitions per case, 18 cases (5,400 calls).
Compilation, fixture population and fresh StateDB creation are outside the timer.
Measured work includes real FFI, SDK cached state, canonical serialization and
staging. SDK backing is in-memory. Rust allocations are not counted by Go B/op.
No disk fsync, IAVL durable commit, networking, consensus, cold database or
representative validator hardware was measured. Debug-build pilot timings are
excluded. Null-filled and printable log payloads have the same encoded gas in
this fixture; rates use actual emitted bytes, not a guessed escape multiplier.

| Map entries / existing record bytes | Operation | Gas | Median microseconds |
| --- | --- | ---: | ---: |
| 0 / 697 | Read | 6,586 | 71.1 |
| 0 / 697 | Add first entry | 220,167 | 93.3 |
| 16 / 5,413 | Read | 34,882 | 388.5 |
| 16 / 5,413 | Overwrite entry | 201,183 | 374.8 |
| 16 / 5,413 | Add entry | 390,063 | 389.3 |
| 64 / 19,621 | Read | 120,130 | 1,018.1 |
| 64 / 19,621 | Overwrite entry | 712,671 | 943.4 |
| 64 / 19,621 | Add entry | 901,551 | 904.4 |
| 64 / 19,621 | 4,096-byte printable event | 199,762 | 1,392.0 |

The `entries0/overwrite` case necessarily inserts its first entry; it is not
evidence of an existing-key overwrite. Gas is deterministic across repetitions;
wall time is machine-dependent. Rates come from the stated baselines/policy,
not fitting gas-per-nanosecond to one laptop benchmark.

Pinned Evmos lab: `45d051dc1fe2f0e9e585b09be50c1df80ec99e24`.
Pinned SDK lab: `f2e6295b662fdb27ea33da1296c29588ccdaab42`.
Rust release FFI SHA-256:
`1dbf0275d6b0c55495ad3223a137000e7832f18346dc1bb58fc960e270d7479c`.
Rust source is unchanged from toolchain `b06c17e`; this candidate changes Go
metering, tests and documentation on top. Final one-pin packaging remains pending.

```sh
cargo +1.96.0 build --release --locked -p lithovm-ffi
cd integration/native-chain
go test -mod=mod -tags=lithovm_chain_lab,lithovm_release -count=1 .
go test -mod=mod -tags=lithovm_chain_lab,lithovm_release -run='^$' \
  -bench=BenchmarkCandidateGas -benchtime=100x -count=3 -cpu=1 .
```

## Consequences and decisions required

1. **@Amir Dev, nominated Foundation technical approver:** confirm or revise KV/log/growth rates for
   the disabled review candidate. Do not equate this with production acceptance.
2. **State scalability:** the current host persists code and all state as one
   record. Charging its real bytes makes token operations more expensive as the
   holder/allowance set grows. The 64-entry results demonstrate this directly.
   Recommend per-key persistent storage and a separately owned native store with
   genesis/upgrade support before a production Finance/LAX rollout; lowering fees
   alone conceals the work instead of removing it. LTH-09 remains open.
3. **Limits:** growth plus Set pricing alone permits at most 15,264 bytes (14.9 KiB)
   of newly allocated serialized key/value data within 10M, before reads, logs
   and VM execution. The 64 KiB bytecode admission bound is not a promise that
   such code can deploy under this gas budget. Foundation must assess useful
   contract sizes and workloads before choosing final limits or serialization.
4. **Validator envelope:** the checked-in Lithosphere genesis declares 100M
   block gas; the later active Makalu capture measured the same ceiling at
   one height on one validator, but not fleet-wide application enforcement. Purely
   extrapolating these cached reads to 100M gives roughly one second of local
   execution, excluding all the unmeasured costs above. This is not headroom
   evidence. Obtain Foundation's validator-class test environment and accepted
   block-time/state-growth budgets; test full blocks, durable state, worst-case
   failed calls, maximum records/logs and long-lived map workloads.
5. Independent reviewers must retest metering, failure paths and affordability,
   including the later [shared live cross-language fuel candidate](NATIVE_PER_KEY_FUEL_CANDIDATE_2026_09_30.md). Keep release, registration,
   activation and live deployment gates closed while these remain outstanding.
