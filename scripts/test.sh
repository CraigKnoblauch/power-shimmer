#!/usr/bin/env bash
# Run the full workspace test suite.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> cargo test --workspace --all-targets"
cargo test --workspace --all-targets "$@"

echo "==> All tests passed."
