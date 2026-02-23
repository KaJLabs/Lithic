#!/usr/bin/env bash
set -euo pipefail
echo "[build] Rust workspace"
cargo build --workspace

echo "[build] Provider reference node"
cd packages/provider/reference-node
if command -v pnpm >/dev/null 2>&1; then
  pnpm install
  pnpm build || true
else
  npm install
  npm run build || true
fi
