# Disabled native-gateway chain binary — isolated build, 2026-10-01

Status: **build/regression evidence only**. No `lithod` process was started;
the gateway remains unregistered. This binary is not the live Makalu binary,
not a release artifact, and not approved for validator rollout or deployment.

The isolated build host used the existing source pins: Evmos lab
`badff8be1a3bb565923ae63fd4f19599da4394d6`, Lithic packages at
`c28cf64f83be25b1ae2ca9a27b3ae77f4f27fc73`, and SDK lab
`f2e6295b662fdb27ea33da1296c29588ccdaab42`. The current Lithic boundary
tests are a subsequent test-only change (`bccb38d`). The tagged `cmd/evmosd`
build initially failed because Evmos assigned to the pinned SDK's
`flags.DefaultGasAdjustment` constant. Evmos commit
`ddfe53fcdfbbddf9919466cd4313a0c42f773965` fixes the command code by
setting the already-registered root flag's value and displayed default to the
existing 1.2 setting. It does not modify the pinned SDK or consensus gas
schedule. A focused regression asserts the value, displayed default, and
unchanged-flag state.

The candidate was built from a copy of the pinned Evmos extraction plus those
two committed command files, with `CGO_ENABLED=1`, Go 1.26.0,
`-mod=readonly`, and `-tags lithovm_chain_lab`. The original extracted archive
was not edited. The binary is
`/var/lib/lithic-bench/candidate-bin/evmosd-lithovm-lab`, 202,769,392 bytes,
SHA-256 `2f34b8ed36c875698e20c6c6f7787fd1784b7351e85cc09b1d7a022593614e0e`.
`ldd` resolves `liblithovm_ffi.so` from the pinned Lithic extraction. The
binary's CLI `version` output is blank because this lab build did not inject
release metadata; its Go build information identifies `go1.26.0` and
`github.com/evmos/evmos/v20/cmd/evmosd`.

Reproduction in the isolated source layout:

```sh
cd /var/lib/lithic-bench/source/pinned-durable-20261001/evmos-candidate-working
CGO_ENABLED=1 go test -mod=readonly -tags lithovm_chain_lab -count=1 \
  ./cmd/evmosd ./app ./x/evm/keeper
CGO_ENABLED=1 go test -mod=readonly -count=1 \
  ./cmd/evmosd ./app ./x/evm/keeper
CGO_ENABLED=1 go build -mod=readonly -tags lithovm_chain_lab \
  -o /var/lib/lithic-bench/candidate-bin/evmosd-lithovm-lab ./cmd/evmosd
```

Both tagged and ordinary suites passed. This only clears the candidate-build
prerequisite. The live Makalu executable is the separate 141,221,080-byte
`lithod` SHA-256 `1f03146df86391715b86971b14b6074580b7efd06d7265a1725d90e426b8efbc`.
The candidate's different executable and synthetic chain identity are
intentional isolation/drift that must be evaluated by Foundation; results
cannot be represented as measurements of the live binary. Before a native
full-block benchmark, Foundation's technical approver must agree the finite
isolated workload envelope and the candidate-binary/config drift to evaluate.
The run must use synthetic keys/state, no live peers, and the captured 100M
Makalu block-gas ceiling. No live-chain change is authorized by this build.
