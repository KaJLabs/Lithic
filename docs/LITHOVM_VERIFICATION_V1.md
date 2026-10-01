# LithoVM artifact and source verification v1

Status: compiler, schema and offline rebuild worker candidate; live indexer wiring pending

## Offline worker

Build with `cargo build -p lithc --bin lithverify --release` and invoke:

```text
lithverify REQUEST.json 700777 0x0000000000000000000000000000000000000007 CHAIN_CODE.bin
```

The indexer must obtain raw native bytecode and identity independently at a
pinned block from its configured chain adapter. Never accept the chain-code
file, chain ID or observed address from the source submitter. Exit zero returns
JSON containing source hash, rebuilt code hash and compiler version; nonzero
returns an error on stderr and no success payload. Both input files are capped
at 4 MiB. Source paths are metadata only and are never opened by the worker.
Run compiler jobs with process CPU/memory/time limits in the hosting service.

The library boundary is `lithic_lithovm::verification::verify`. It rejects
unknown/duplicate JSON fields, noncanonical identities, multiple sources,
source-hash mismatch, compiler-version mismatch, modified artifact metadata,
and any difference between rebuilt and independently observed bytecode.
The compiler version alone is not a release identity: production workers must
pin an approved executable digest/provenance as well. This local candidate has
not been published as a production compiler release.

`lithc --emit lithovm` emits a versioned JSON artifact containing the exact
compiler version, target, bytecode version, raw-source Keccak-256, bytecode
Keccak-256, ABI, canonical entrypoints and bytecode. JSON field order is not a
consensus property; the hash values and bytecode are.

The source hash is computed over the exact UTF-8 bytes supplied to the
compiler. Verification clients must not normalize line endings, Unicode or
trailing whitespace. Each selector is the full 32-byte Keccak-256 digest of:

```text
function_name(type_name,...)
```

For example, `transfer(address,u256)` is
`0xf61367304e4e32065cad538b10a44bb599f78e0771530108572d225c74122c1f`.

## Schemas

- `sdk/schemas/lithovm.artifact.schema.json` describes compiler output.
- `sdk/schemas/lithovm.verification.schema.json` packages exact source bytes,
  artifact, chain ID, canonical 20-byte address and deployed code hash.
- `sdk/schemas/lithovm.deployment-status.schema.json` is the shared
  litho.finance/LithoScan status envelope.

Sources must be sorted by path before submission. Native v11 currently accepts
one source, and its source hash must equal both the source entry hash and the
artifact `sourceHash`. A verification worker must select the exact released
compiler version, compile the exact content, validate the complete artifact,
and accept only when rebuilt bytecode and `codeHash` equal chain state. Merely
matching an ABI or contract name is insufficient.

The lifecycle is monotonic:

```text
prepared -> submitted -> included -> verified
                |            |
                v            v
              failed   verification_failed
```

`failed` records transaction/deployment failure. `verification_failed` means a
contract was included but its submitted source package did not reproduce the
deployed code. Initializer success is part of atomic inclusion, not a separate
state.

The schemas do not define wallet authorization, broadcast, finality depth,
retry policy or database ownership. Those remain frontend, chain RPC and
LithoScan integration gates. Nothing in this interface authorizes deployment.
