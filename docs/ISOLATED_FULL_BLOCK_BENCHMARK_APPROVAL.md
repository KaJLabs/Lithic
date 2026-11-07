# Approval request: isolated native full-block benchmark

Status: **approved with conditions for the finite isolated synthetic benchmark only**
on 2026-10-01 by Amir Dev for Lithosphere Foundation. Results require a
separate Foundation review. This is not production authorization.
Technical approver: Amir Dev for Lithosphere Foundation. Host acceptance by
Alex covers the isolated 13-vCPU/78-GB/350-GB VPS, not this workload or its
results.

## Exact scope to approve or revise

- Use the [disabled tagged chain candidate](DISABLED_CANDIDATE_BINARY_2026_10_01.md)
  only on the isolated VPS, with a synthetic chain ID, synthetic validator/test
  keys created there, no live Makalu keys/state/peers, and loopback-only
  services. Never attach this binary to Makalu or advertise public RPC.
- Keep the production gateway disabled. Exercise the tagged lab native route
  only inside the synthetic chain after its lab store-upgrade rehearsal. No
  LAX or production contracts, funded transactions, bridge, or MultX traffic.
- Use the [captured live Makalu](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md)
  100,000,000 gas/block and 21,000,000 bytes/block as hard test ceilings;
  stop below any stricter candidate application limit discovered in preflight.
  Do not change live consensus parameters.
- Run **100 consecutive blocks per utilization point** at nominal 10%, 25%,
  50%, 75%, and 100% of the permitted block-gas ceiling. Include a warm-state
  pass and a separately controlled cold-cache pass; a process restart alone
  does not evict the OS page cache. Keep synthetic state at 1k, 10k,
  and 100k holders, equivalent allowances, and mixed state. Include deploy,
  read, transfer/overwrite/new growth, approve/allowance, mint/burn, maximum
  state transitions, small/4-KiB/maximum logs, and failed/OOG/reverted calls.
  Record actual achieved gas and bytes per block; a nominal target is not a
  claim that transactions can fill it.
- Measure p50/p95/p99 execution and durable commit time, CPU/RSS, physical DB
  reads/writes, and logical/persistent growth. Preserve raw traces, exact
  binary/FFI/source/config hashes, restart boundary and failed-call counts.
  Stop if a block exceeds the observed ceiling, state diverges, the process
  fails/restarts unexpectedly, or the isolated host shows resource exhaustion.

The disabled per-transaction [token-state](DURABLE_TOKEN_STATE_BENCH_2026_10_01.md)
and [deployment/log](DURABLE_BOUNDARY_CANDIDATE_2026_10_01.md) runs are
preflight evidence only. In particular, 16/32/64-KiB code passed gateway
admission but could not deploy under the provisional 10M native cap. This
benchmark cannot by itself approve that growth price/cap or the gap between
admission and deployability. Foundation must review the results and set final
execution-time, state-growth and pricing budgets separately.

The read-only [pre-key safety preflight](../scripts/check-isolated-lithovm-bench.py)
passed on the isolated VPS on 2026-10-01. It verified the candidate binary
and synthetic config hashes recorded above, the distinct chain ID, the
100M/21M ceilings, disabled public services, loopback-only P2P/RPC,
zero configured peers and absence of key files. Its six focused checks and
the four existing preparation checks pass locally. The preflight does not
start a node, create synthetic validator keys, or establish workload approval.

The approval requires the tagged binary only on the isolated host, a distinct
synthetic chain ID and keys, loopback-only P2P/RPC/services, no Makalu keys,
state or peers, no production traffic or assets, no live consensus change,
all listed workloads and measurements, and immediate stop on a ceiling
violation, state divergence, unexpected process failure/restart or resource
exhaustion. The tagged binary differs from the live Makalu executable.
Approval does not establish a native gas cap, growth price, deployability
policy, registration, activation, validator rollout, LAX, MultX, bridge or
production custody permission.
