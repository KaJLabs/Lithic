# Separate native-store lifecycle — disabled lab evidence

Status: **isolated, build-tagged candidate only**. No Makalu or mainnet
registration, upgrade, state migration, deployment or gateway activation.
The local Evmos lab implementation is pinned at
`24e674de1bd4150025562aa6e393a672736feb65` (parent
`45d051dc1fe2f0e9e585b09be50c1df80ec99e24`).

The Evmos lab build with `lithovm_chain_lab` mounts a dedicated `lithovm` KV
store. The keeper passes this store—not the EVM module store—to the disabled
gateway. If it is absent, the lab gateway fails closed. The ordinary build
mounts no `lithovm` store, installs no gateway, and rejects a LithoVM genesis
section. Keeper tests verify that simulation and estimation leave no native
record and that a committed native record appears only in the dedicated
store, never in the EVM store.

Genesis has a versioned `lithovm` section with sorted key/value entries.
Export validates canonical native keys, version-2 contract records, the code
hash, scalar-only contract storage, fixed 32-byte map values and absence of
orphan map entries. Import fully validates and bounds the snapshot before
writing, rejects duplicates/out-of-order keys, requires an empty destination,
and leaves no partial writes on invalid input. The tagged app exports this
section and imports it during fresh InitChain; a missing section is allowed
only if the native store is empty. The ordinary app rejects the section rather than silently
discarding it. This snapshot codec has a lab ceiling of one million entries
and 512 MiB of decoded key/value data; that is **not** an approved production
scalability envelope.

Tests on the isolated VPS working copies passed on 2026-09-30:

```text
Lithic native-chain:  go test -mod=mod -tags lithovm_chain_lab -count=1 .
Evmos tagged:         go test -mod=mod -tags lithovm_chain_lab -count=1 ./x/evm/keeper ./app
Evmos ordinary:       go test -mod=mod -count=1 ./x/evm/keeper ./app
Evmos targeted race:  go test -mod=mod -race -tags lithovm_chain_lab -run TestKeeperTestSuite/TestLithoVMCandidate -count=1 ./x/evm/keeper
```

The native-chain suite additionally tests cross-frame read-your-writes and
parent rollback, a nonempty genesis round trip, malformed/duplicate/orphan
snapshots, and no partial import. The app suite covers both empty and
nonempty native-state export/import. These tests use isolated/in-memory
stores, not durable validator-class block commits.

Adding a new mounted store to a chain with existing state requires a named,
height-governed store upgrade and a reviewed deployment plan. **No such
Makalu upgrade is implemented or approved here.** A tagged binary must not
be substituted for a running validator binary. Genesis export/import is for
a fresh isolated chain or explicitly approved state export; it is not a
hot migration of preview whole-record state. Version-1 inline-map records
remain rejected and would need a separate migration if ever encountered.

The [disabled store-loader rehearsal](NATIVE_STORE_UPGRADE_REHEARSAL_2026_10_01.md)
now adds a height-gated lab plan and tests state preservation, but also
demonstrates that an old binary can reopen the post-upgrade database; mixed-
version operation remains unsafe. The next gates are a Foundation-reviewed
coordinated cutover with pre-upgrade snapshot rollback and
mixed-version protection, durable 1k/10k/100k holder and allowance
benchmarks on approved hardware, final gas economics, independent security
retesting, source pinning and Makalu end-to-end approval.
