# Lithic Lang Public Framework

Framework for **Lithic Lang** contract authoring, compiler/runtime design, and LEP100-aligned reference implementations.


## What is included

- Public compiler/runtime architecture docs
- Module-by-module specs for the authoring stack
- LEP100/LSCL-style standards scaffolding
- Example `.lithic` contracts for fungible tokens, NFT, AI metering, provenance receipts, budget escrow, bridge mint/burn, and privacy-preserving account linking
- ABI, manifest, storage, and event schemas
- CI starter workflow
- Test/spec placeholders for conformance and simulations

## Repository layout

```text
lithic-lang-public-framework/
  .github/workflows/ci.yml
  README.md
  docs/
    architecture/
      compiler-runtime-overview.md
      security-model.md
      package-lifecycle.md
    specs/
      abi-spec.md
      manifest-spec.md
      storage-layout-spec.md
      event-schema-spec.md
      module-specs.md
      roadmap.md
  schemas/
    contract.abi.schema.json
    contract.manifest.schema.json
    contract.storage.schema.json
    contract.events.schema.json
  contracts/
    standards/
      lep100_ft.lithic
      lep100_nft.lithic
      ai_provider_registry.lithic
      budget_escrow.lithic
      provenance_verifier.lithic
      zk_verifier_registry.lithic
      bridge_mint_burn.lithic
      ppal_registry.lithic
    examples/
      treasury_agent.lithic
      ai_metered_service.lithic
      marketplace_hook_demo.lithic
      shared_asset_demo.lithic
  stdlib/
    README.md
  tooling/
    bin/
      lithc
      lithfmt
      lithlint
      lithdev
  tests/
    conformance.md
    simulation-plan.md
```

## Design goals

1. Deterministic smart-contract execution for LithoVM-style environments
2. First-class support for LEP100 asset and service standards
3. AI-native execution flows with signed receipts and budget settlement
4. Capability-based security model
5. Formal-verification-friendly metadata and compilation outputs
6. GitHub-friendly documentation and spec-first development

## Target toolchain

The framework is designed around the following public-facing tools:

- `lithc` — compiler
- `lithfmt` — formatter
- `lithlint` — linter and static analyzer
- `lithls` — language server (spec only in this scaffold)
- `lithdev` — local devnet and deployment helper
- `lithtest` — test runner (spec only in this scaffold)
- `lithsec` — capability and storage safety scanner (spec only in this scaffold)
- `lithpkg` — package manager (spec only in this scaffold)

## Suggested next steps

- Replace example scripts in `tooling/bin` with a real parser/typechecker/backend
- Add bytecode generation and a LithoVM adapter
- Implement package manager and language server
- Add reference test vectors for Makalu
- Audit the contracts before mainnet deployment

## License

Apache 2.0
