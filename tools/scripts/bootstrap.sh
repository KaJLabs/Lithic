#!/usr/bin/env bash
set -euo pipefail

echo "[bootstrap] Checking toolchains..."
command -v cargo >/dev/null 2>&1 || { echo "Rust is required"; exit 1; }
command -v node >/dev/null 2>&1 || { echo "Node.js is required"; exit 1; }

echo "[bootstrap] OK"
