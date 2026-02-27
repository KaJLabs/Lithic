# Lithic Ecosystem

This repository is a **production-friendly, enterprise-ready** scaffold for the Lithic ecosystem on Lithosphere/LithoVM.
It cleanly separates:

- Core language + toolchain (`packages/compiler/*`)
- LithoVM runtime + bytecode/receipts (`packages/vm/*`)
- LSCL secure contracts library (`packages/lscl/*`)
- AI provider infrastructure + SDKs (`packages/provider/*`)
- zk-verifiable extension assets (`packages/zk/*`)
- Testing/devnet/mocks (`packages/testing/*`)
- Documentation site source (`apps/docs/*`)

> This is a **starter codebase** designed to compile, lint, test, and run CI end-to-end.
> Replace stubs with full implementations as the spec matures.

## Quickstart

### Prereqs
- Rust (stable)
- Node.js 18+
- pnpm (recommended) or npm
- Docker (optional, for devnet/provider compose)

### Build
```bash
./tools/scripts/bootstrap.sh
./tools/scripts/build-all.sh
```

### Run the reference provider
```bash
cd packages/provider/reference-node
pnpm install
pnpm dev
```

### Compile a Lithic contract (stub compiler)
```bash
cargo run -p lithc -- compile ./apps/examples/zk-required-contract/contracts/ZkRequiredExample.lithic --out ./out
```

### Run docs locally (minimal)
```bash
cd apps/docs
pnpm install
pnpm dev
```

## Repository Layout
See `docs/REPO_STRUCTURE.md` for details.
