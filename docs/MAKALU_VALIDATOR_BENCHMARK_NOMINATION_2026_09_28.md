# Makalu limits capture and isolated Lithic benchmark nomination

Status 2026-09-30: **an isolated VPS is available for build/test preparation;
the active-node limits capture is complete, but validator benchmark acceptance
remains pending.** This
document does not authorize chain configuration changes,
gateway registration/activation, deployment or public load tests.

## What was checked read-only

- Repo inventory identifies Makalu testnet (`chain_id=700777`) primary validator
  `lithod-mtest-val-02` on the shared `srv02` host, home
  `/var/lib/litho-mtest-val-02`, CometBFT RPC port 26757 and EVM RPC port 8645.
  These are inventory claims, **not observed running configuration**.
- The [Foundation network-access page](https://access.litho.ai/network) lists
  `https://rpc.litho.ai` for Makalu testnet (EVM chain 700777).
  Public `https://rpc.litho.ai/status` and `/consensus_params` timed out from
  this workspace. The documented public front `vps1` was reachable by SSH;
  its live nginx routes Makalu to `31.97.39.138:26857` with backup
  `72.60.177.106:26857`. Both upstream read-only requests timed out from vps1.
- SSH from this workspace and from vps1 to the documented primary host was
  refused for the available public key. SSH to the backup host worked, but
  `lithod-makalu-sentry` was inactive. No validator key, app secret, full TOML,
  process environment or private network payload was read.
- Repo Ansible declares `consensus_max_gas: 100000000`; that value cannot be
  represented as live Makalu truth based on repository data alone. A later
  [running-node capture](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md)
  independently observed the same numerical limit on Makalu. This is distinct
  from the 9005 mainnet's height-pinned observation in the gas review.

## Operator-side capture (read-only)

The operator of the **currently running** Makalu validator should first confirm
its actual service name, home and local ports. On that host, run
[`scripts/capture-makalu-effective-limits.sh`](../scripts/capture-makalu-effective-limits.sh)
with those parameters. It prints only service state, binary path/hash/version,
hashes of the config/genesis files, public consensus and EVM block limits, and
selected non-secret app options. It does **not** dump validator/node keys or full
configuration/environment. If `jq` is unavailable, use an operator-approved
equivalent read-only capture; do not install packages or restart the validator
for this request.

Example for the documented service (operator must verify before running):

```sh
bash scripts/capture-makalu-effective-limits.sh \
  lithod-mtest-val-02 /var/lib/litho-mtest-val-02 \
  http://127.0.0.1:26757 http://127.0.0.1:8645
```

The documented primary service was confirmed active on 2026-09-30, and the
[capture evidence](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md)
records its exact running binary, config hashes, and live 100M gas / 21M-byte
consensus block limit. Foundation should still confirm any additional enforced
app limit not exposed by `consensus_params` or the EVM header. No public traffic
was used for this capture.

## Nominated fresh VPS: `lithic-makalu-bench-01`

Requested role: new dedicated **8 vCPU / 32 GiB RAM / 500 GB NVMe** VPS, billed
and owned by Lithosphere Foundation. This is a procurement nomination, not an
existing host assignment or purchase request. Do **not** repurpose Makalu's
bonded validator, public sentries, explorer, mainnet nodes or MultX equipment.

The Foundation operator should choose the provider/region and return the private
host identifier, CPU model, actual usable RAM/NVMe size, filesystem/DB backend,
OS/kernel, and assigned benchmark operator. Provisioning/payment requires their
normal authorization; no provider SKU, IP or account access has been assumed.
No secrets should be sent in chat.

Acceptance before measurements:

1. Verify the running Makalu binary SHA-256; install that exact binary on the
   isolated VPS, not merely a similarly named release. Record both hashes.
2. Reproduce the execution-relevant Makalu genesis/consensus, app/EVM, gas and
   pruning/storage configuration. Record source/destination hashes and a
   redacted comparison of effective settings. Network identity, local ports,
   peer lists and signing settings **must differ** to preserve isolation; this
   exception is not an execution-config drift waiver.
3. Never copy `priv_validator_key.json`, `priv_validator_state.json`, `node_key.json`,
   wallet keys, KMS config or seed phrases. No live peers, signing duties,
   public RPC, faucet/bridge/MultX traffic or funded transactions. Private
   access for approved operators only. Use synthetic/replayed workloads and
   durable local state on the NVMe disk.
4. Run the Amir workload matrix in
   [his recorded review](AMIR_GAS_REVIEW_RESPONSE.md): 4/16/32/64 KiB deployment;
   1k/10k/100k holder and allowance states (after scalable persistence);
   read/write/transfer/approval/mint/burn, OOG/revert, max logs and full blocks.
   Measure p50/p95/p99 execution and commit time, CPU/RSS, DB I/O and persistent
   growth at multiple utilization levels. The active-node capture observed a
   finite 100M Makalu consensus ceiling; agree the finite isolated-test
   envelope and any additional app limits with Foundation first.

## 2026-09-30 VPS preparation and variance

The user supplied a dedicated, idle VPS for this role and directed its use after
reporting a provider upgrade to **13 vCPU / 78 GB RAM / 350 GB NVMe**. A pinned
SSH host-key check and read-only guest inspection at 12:16 UTC instead observed
**11 online vCPU, 48,277,076 KiB (~46.0 GiB) RAM, and 300 GB aggregate virtual
disks** (25 + 100 + 125 + 50 GB); the ext4/LVM root filesystem reports 294 GB.
The guest identifies KVM/QEMU virtual disks, not the underlying provider medium.
CPU and RAM exceed the nominated minima, but the guest does not yet show the
provider-reported size, and neither 300 GB observed nor 350 GB reported meets
the original 500 GB nomination. Confirm the final provider/guest resources and
NVMe backing before representing this as a matching validator-class host. The
user's instruction to use this host permits preparatory tests, not an inference
that Foundation has accepted the storage variance or final performance budgets.
At 12:41 UTC, after provider confirmation was relayed, a second guest inspection
still showed 11 vCPU, 48,277,076 KiB and the same four 300 GB virtual disks.
The isolated idle host was rebooted once at 12:42 UTC to apply any pending
guest-visible resize; a post-reboot check showed no hardware change. Provider
confirmation did not reconcile the guest measurements at that time. A later
provider-side fix and reboot **did** resolve the discrepancy: at 13:41 UTC the
guest showed **13 online vCPU, 78,193,520 KiB (~74.6 GiB) RAM, and five virtual
disks totaling 350 GiB** (25 + 100 + 125 + 50 + 50 GiB). The ext4/LVM root
filesystem reports 343 GiB, with 322 GiB available. The guest still labels the
devices `QEMU HARDDISK`; NVMe backing is provider-confirmed, not independently
visible from inside the VM. The original 500 GB nomination remains a storage
size variance requiring Foundation acceptance for final validator evidence.

The host had no Lithic/LithoVM process, containers or public application listener.
Baseline build packages were installed, a dedicated unprivileged `lithicbench`
account and private `/var/lib/lithic-bench` workspace were created, and Rust
1.96.0 plus Go 1.26.0 were installed. A committed-source archive of Lithic
`df2d4fe03cb69ea5d35b628ccbc2822c6d274618` was hash-verified on upload
(SHA-256 `7a766af65b80974f6520f3aed37a6f88d6b65425ff5e6e206d6f1e282d196984`).
As `lithicbench`, `cargo test --locked -p lithovm-ffi -p lithovm-host --quiet`
passed 40 tests and `cargo build --release --locked -p lithovm-ffi --quiet`
succeeded; the resulting FFI library SHA-256 is
`3a5189c67ae063f7841f0a98860205a1305892aaf5194f1e2d19268e7f9de6c3`.
These are compilation/regression checks, **not** durable-state or full-block
benchmark evidence. No chain binary/config, validator key or public traffic was
copied. An initial read-only `/status` and `/consensus_params` probe to the
documented public Makalu RPC timed out from this VPS. A later read-only
operator capture from the active validator established the effective live
limits; see [the pinned capture](evidence/MAKALU_EFFECTIVE_LIMITS_CAPTURE_2026_09_30.md).

The host's provider-reported CPU/RAM/disk size is now guest-visible. The
hash-matched live binary is staged, and a
[sanitized isolated baseline config](MAKALU_ISOLATED_BENCH_CONFIG_2026_09_30.md)
is prepared but not started. Before the acceptance matrix can run, obtain
Foundation acceptance of the 350 GiB storage variance and the agreed finite
test envelope, then finish
scalable per-key persistence plus shared Go/Rust metering.
The current WSL2/in-memory results and this host's smoke checks do not satisfy
the validator-class acceptance gate.
