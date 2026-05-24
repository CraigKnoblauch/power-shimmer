#!/usr/bin/env bash
# Run formatting, Clippy, and optional cargo-deny checks.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt --check"
cargo fmt --all -- --check

if command -v taplo >/dev/null 2>&1; then
    echo "==> taplo fmt --check"
    taplo fmt --check
else
    echo "==> taplo not installed — skipping TOML format (install: cargo install taplo-cli)"
fi

if cargo clippy --help >/dev/null 2>&1; then
    echo "==> cargo clippy --workspace --all-targets --all-features"
    cargo clippy --workspace --all-targets --all-features -- -D warnings
else
    echo "==> cargo clippy not installed — skipping (install: rustup component add clippy, or apt install clippy)"
fi

if command -v cargo-deny >/dev/null 2>&1; then
    echo "==> cargo deny check"
    cargo deny check
else
    echo "==> cargo deny not installed — skipping (install: cargo install cargo-deny)"
fi

echo "==> Lint checks passed."
