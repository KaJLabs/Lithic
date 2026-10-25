# Combined compiler and native-runtime release process

The release includes `lithc`, `lithverify`, `lithfmt`, `lithlint`, the native
`lithovm-ffi` shared library and its C header. All are built from one source
snapshot with the exact Rust version in `rust-toolchain.toml` and `Cargo.lock`.
Python 3.11 or newer, git and the platform's Rust/native linker are required.

## Local reproducibility and installation check

On a native Linux x86_64 host, run:

```sh
python3 -B scripts/release_toolchain.py build \
  --version lithic-v0.2.0-production-candidate.1 \
  --target x86_64-unknown-linux-gnu --output /tmp/lithic-release-candidate --offline
```

The destination must be new and outside the source checkout. A dirty checkout
is rejected unless `--allow-dirty` is explicitly supplied for a local candidate;
the resulting build evidence records that state and a hash of every source
input. It must not be represented as a release from the clean Git commit.

The script builds twice in independent source/target directories with path
remapping and a source-commit timestamp. It compares all five executable/library
payloads byte-for-byte and fails without emitting an archive on any difference.
It then runs the packaged compiler and native FFI v2 together: compile a small
contract, deploy into callback-backed local state, call and verify the result,
check live fuel callbacks and reject an invalid call without durable effects.
The packaged `lithverify` rebuilds that exact source and rejects modified code.
This smoke never registers a chain service, signs a transaction or uses LAX.

The archive includes deterministic metadata, a complete file/hash manifest,
source inventory, Cargo lockfile, CycloneDX 1.5 dependency graph, build comparison
and smoke evidence. The SBOM describes Cargo's resolved workspace graph,
including optional/development dependencies; it is not a vulnerability scan or
proof that every listed dependency is linked into every binary.

```sh
python3 -B scripts/release_toolchain.py verify /tmp/lithic-release-candidate/<archive>
sha256sum -c /tmp/lithic-release-candidate/<archive>.sha256
```

Run the checksum command from the archive directory (or adjust its relative
filename). Use the corresponding native target on Windows x86_64/MSVC or macOS
Apple Silicon. Cross-compilation is rejected because the packaged smoke must
execute on the target host. Unix archives use normalized tar/gzip metadata;
Windows archives use sorted ZIP members with fixed timestamps and permissions.

Reproducibility here means identical payloads from two distinct build roots on
the same host with the same toolchain/dependencies. It does not establish
cross-platform equality, independence from OS/linker upgrades or independent
third-party reproduction. Any unsupported platform comparison must fail rather
than be waived by the packaging script.

## Reviewed GitHub release

1. Review the combined source changes, security findings, dependency/SBOM
   results and compatibility evidence. Resolve the remaining production gates.
2. Authorize an annotated `lithic-v<version>` tag and the signing identity/policy.
   A locally checksummed archive is not a signed release. If a signed Git tag is
   used, verify it against the approved key; do not invent a signing identity.
3. The pinned release workflow runs Rust fmt/clippy/workspace tests, packaging
   regressions and independent rebuild/smoke on Linux, Windows and Apple Silicon.
4. GitHub signs build-provenance attestations for the exact resulting archives.
   Verify against the approved repository/workflow and source tag:

   ```sh
   gh attestation verify <archive> --repo KaJLabs/Lithic
   ```

5. Verify checksums, file inventory, source pins, smoke and SBOM evidence.
   Review the draft release before authorizing publication. Existing assets
   are not silently overwritten by a rerun.

The script and workflow do not themselves establish production acceptance,
chain activation, validator rollout, contract deployment or asset custody.
