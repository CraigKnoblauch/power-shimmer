#!/usr/bin/env bash
# Full local CI gate: format, lint, build, test.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/verify-rust.sh"
echo ""

"$ROOT/scripts/lint.sh"
echo ""

echo "==> cargo build --workspace --all-targets"
cargo build --workspace --all-targets
echo ""

"$ROOT/scripts/test.sh" "$@"

echo "==> All checks passed."
