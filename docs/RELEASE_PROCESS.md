# Combined compiler and native-runtime release process

The release includes `lithc`, `lithverify`, `lithdev`, `lithfmt`, `lithlint`, the native
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

Clean releases use the `raw-git-blobs-v1` input policy: every input must match
its raw Git blob in `HEAD`, including its bytes and the complete file inventory.
Text checkouts use LF on every platform through `.gitattributes`; explicitly
binary fixtures remain byte-preserved. A clean Git status alone is insufficient:
the packager rejects checkout conversions before compiling. Build evidence records
the exact source commit, tree and policy. Dirty local candidates are labelled
`working-tree-local-v1` and `source_matches_commit: false`.

The script builds twice in independent source/target directories with path
remapping and a source-commit timestamp. It compares all six executable/library
payloads byte-for-byte and fails without emitting an archive on any difference.
It then runs the packaged compiler and native FFI v2 together: compile a small
contract, deploy into callback-backed local state, call and verify the result,
check live fuel callbacks and reject an invalid call without durable effects.
The packaged `lithverify` rebuilds that exact source and rejects modified code.
The packaged `lithdev` prepares native deployment/call intents from the same
source and artifact; see [its usage and boundaries](LITHDEV_NATIVE_PREPARATION.md).
This smoke never registers a chain service, signs a transaction or uses LAX.

New development packages declare smoke profile `native-account-v17-bank-candidate-v1`.
In addition to the legacy check, the packaged compiler compiles the proposed
native account and the packaged FFI initializes it, reloads its binary record
and signer map, rejects unauthorized/repeated initialization, checks OOG and
recovery, and rejects a corrupt stored code hash. A separately compiled fixture
verifies a valid public signature and rejects a modified digest. The package
includes only the public digest/signature/address, bound to the unchanged
proposed vector-file hash; no private-key fields are copied. The source worker
rebuilds the actual v17 account and rejects modified code, while `lithdev`
rejects preparing that account for the documented Makalu v14 profile.

The new profile also compiles a custody fixture and exercises the packaged FFI
v3: preserve prefunding, reject unauthorized/reverting/insufficient/OOG calls
without effects, and return a supply-conserving bank settlement without applying
external balances. This is a separately disabled
[custody candidate](NATIVE_CUSTODY_CANDIDATE.md), not approval to replace the
first nonpayable chain profile. Historical `native-account-v17-v1` archives keep
their explicit v2-only smoke behavior.

The manifest records the smoke profile and requires its fixtures. Historical
archives without this field retain their legacy validation rules. Running
`smoke --directory <extracted-package> --target <native-target>` selects the
manifest's profile automatically; a contradictory `--profile` is rejected.
This is package execution evidence for a proposed development profile, not
standards certification, chain integration or benchmark acceptance. Reported
initialization VM gas excludes keeper KV/growth/log and outer transaction costs.

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
MSVC builds use `/Brepro` so PE timestamps and CodeView identifiers are
deterministic. The macOS FFI is relinked with the stable install name
`@rpath/liblithovm_ffi.dylib`, instead of an absolute temporary build path.
These are linker settings, not post-build binary rewriting. If payloads still
differ, the build fails and retains the actual failing payloads/metadata in a
separate `*-failed-rebuild` directory; those files are not release candidates.

Reproducibility here means identical payloads from two distinct build roots on
the same host with the same toolchain/dependencies. It does not establish
cross-platform equality, independence from OS/linker upgrades or independent
third-party reproduction. Any unsupported platform comparison must fail rather
than be waived by the packaging script.

## Reviewed GitHub release

The separate `release-checks.yml` workflow runs native Linux, Windows and macOS
rebuild comparisons and packaged execution checks for compiler/runtime PRs.
The Apple Silicon build host is `macos-15`; GitHub retired the `macos-14`
Actions images on 2 November 2026 ([runner-image notice](https://github.com/actions/runner-images)).
It retains unsigned candidates with their embedded source/build/smoke evidence.
PR builds check out the selected head commit directly, rather than a synthetic
CI merge. The review packet must include that exact build commit and its tree
and parents in the Git bundle, alongside the outer archive checksum sidecar.
Its read-only repository token cannot publish a GitHub release. These checks
provide review evidence before any release tag or publication decision.

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
