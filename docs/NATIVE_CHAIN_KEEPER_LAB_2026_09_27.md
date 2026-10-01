# Disabled LithoVM keeper lab (2026-09-27)

Status: local signed-transaction integration evidence, **not** a Makalu or
mainnet deployment, release artifact or activation approval.

The isolated Evmos worktree `litho-native-chain-lab` is on branch
`feat/lithovm-disabled-gateway-lab` at `0e2522d`. It starts at pinned Evmos
`eca13ef2521a9ef13c32e80b1b147230bdb155b5` and contains the six
hash-verified LITHO release patches plus lab-only LithoVM changes. Its `go.mod`
uses local sibling replacements for the pinned SDK and Lithic test module; this
branch is **not** a portable production release definition. The immutable
Lithosphere release manifest and running chain were not modified.

The ordinary build has a no-op hook. Under `lithovm_chain_lab`, the keeper's
`ApplyMessageWithConfig` supplies message/commit context to a call hook. The
hook re-registers the native gateway on each gateway call because Evmos may
replace its active precompile map when another precompile is called. The
gateway obtains sender and nonce from `core.Message`, chain ID from chain
configuration, and height/time from EVM block context; calldata cannot supply
those fields. Fake simulation messages cannot commit.

Tests on the pinned Go 1.22.12/Linux toolchain passed:

```sh
CGO_ENABLED=1 go test -mod=mod -count=1 ./x/evm/keeper
CGO_ENABLED=1 go test -mod=mod -tags lithovm_chain_lab -count=1 ./x/evm/keeper
CGO_ENABLED=1 go test -mod=mod -race -tags lithovm_chain_lab \
  -run TestKeeperTestSuite/TestLithoVMCandidate -count=1 ./x/evm/keeper
```

The tagged keeper test signs a local EVM message, deploys a pinned compiled Lithic
counter via the gateway, observes a receipt log and persisted native record,
confirms a simulation discards the record, confirms a native failure leaves
storage unchanged, and confirms a later successful call updates it. A malformed
call through the full keeper transition fails; a call to an unregistered empty
account would instead succeed. The ordinary build test verifies the hook is
absent. A pre-existing keeper deployment fixture used a module address and
failed the pinned module-account guard; the lab test now uses a funded EOA,
without weakening that guard. Both full keeper profiles pass afterward. The
Counter fixture embeds bytecode compiled at Lithic `4de97ab`; its documented
`sourceHash` is Keccak-256, not the file's SHA-256. The fixture no longer needs
a sibling compiler executable at test runtime.

This does not exercise signed broadcast through Makalu RPC, consensus replay
across nodes, upgrades, real receipt indexing/finality, payable transfers,
native-to-EVM calls, production gas, or independent security review. The next
release candidate needs a separately reviewed source patch/manifest, pinned
toolchain and reproducible build, then disabled-node and Makalu rehearsals.
