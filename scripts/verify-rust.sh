#!/usr/bin/env bash
# Verify the active Rust toolchain meets workspace MSRV before building.
set -euo pipefail

MSRV="1.75.0"

if ! command -v rustc >/dev/null 2>&1; then
    echo "error: rustc not found on PATH" >&2
    exit 1
fi

RUSTC_VERSION="$(rustc --version | awk '{print $2}')"

version_ge() {
    # Returns 0 when $1 >= $2 (semver tuple compare).
    local IFS=.
    local i ver_a=($1) ver_b=($2)
    for ((i = 0; i < 3; i++)); do
        local a=${ver_a[i]:-0}
        local b=${ver_b[i]:-0}
        if ((10#$a > 10#$b)); then return 0; fi
        if ((10#$a < 10#$b)); then return 1; fi
    done
    return 0
}

if ! version_ge "$RUSTC_VERSION" "$MSRV"; then
    echo "error: Rust $MSRV or newer is required (found $RUSTC_VERSION)." >&2
    echo "Install via rustup: https://rustup.rs" >&2
    exit 1
fi

echo "==> Rust $RUSTC_VERSION (MSRV $MSRV)"
