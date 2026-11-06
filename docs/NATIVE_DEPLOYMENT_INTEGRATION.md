# Native deployment integration candidate — 6 November 2026

This branch adds offline receipt/source-worker support on top of the frozen
`b1ea60a0341d8f0fa0dbb3290c1d0a81a7eb5794` v17 candidate. It does not change the
reviewed archive, runtime bytecode, C ABI, gas schedule or deployed chain build.

`lithdev decode-receipt RECEIPT.json --chain-id ID --gateway ADDRESS` accepts
`lithovm-evm-receipt-v1` observations from an operator-configured trusted reader.
It strictly decodes the gateway's raw JSON log payload, derives deployment
addresses and numbers creations across the entire EVM transaction. A reverted
outer receipt produces no native registrations or events. Duplicate registrations,
malformed payloads and inconsistent origin metadata fail the complete batch.
Foreign emitters cannot supply registrations. Limits apply before decoding.

Receipt input contains canonical decimal strings for chain ID, nonce, height,
gas used and log index; lowercase 0x-prefixed hashes/addresses/data; outer status
`success` or `reverted`; and log address/topics/data/index. The CLI is offline:
the embedding adapter authenticates RPC, runtime and canonical block identity.
`canonicalProofVerified` always remains false. Observation is not finality.

`lithverify --native-record REQUEST.json CHAIN_ID ADDRESS RECORD.bin` accepts the
independently retrieved native KV record at that block. It reuses the FFI's
existing v2/v3 record decoder, validates code hash, bytecode and scalar-storage
encoding, then performs the existing exact source/artifact/code comparison.
The old raw-code invocation is unchanged. A source match establishes neither
state correctness nor consensus inclusion, physical durability or finality.

The companion Litho Finance adapter binds the prepared factory request to the
wallet, nonce, reviewed code/runtime, salt and every token setting. It estimates
and simulates before one submission, verifies the returned transaction, checks
the host registration and TokenCreated metadata, and retains a submitted hash
when confirmation is unavailable. LithoScan's companion route reads receipts
and native code independently, runs pinned local workers, and rechecks canonical
block identity before saving/publishing a source match. Both live integrations
remain disabled until their owner-configured chain profile is installed.

`scripts/generate_native_integration_fixture.py --binary-dir DIR --output FILE`
creates the shared synthetic compiler/FFI fixture. It deploys the real Finance
token/template and factory, creates an initialized token with caller-bound salt,
decodes the registration and verifies source against the resulting native KV
record. No network, signer or broadcast is used. Binary hashes in that fixture
identify its local producer; cross-platform test results compare semantic
outputs, not platform binary hashes. The fixture is copied unchanged into the
compiler, frontend and explorer test suites.

The new receipt fuzzer and CLI regressions cover malformed input, reverted
receipts, transaction-wide IDs and corrupted records. Existing Makalu intent
preparation still rejects bytecode above its documented v14 profile.

The accepted R6.4 diagnostic closeout and failed 50M benchmark are unchanged.
No node, remote workload, contract deployment, LAX or MultX action is included.
