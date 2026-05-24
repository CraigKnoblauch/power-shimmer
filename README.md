# Power Shimmer

A lightweight desktop utility that plays a rainbow shimmer overlay when your laptop transitions from battery to AC power.

**Status:** v1 Linux (X11 + UPower) — core orchestration, power adapters, GPU overlay, and application shell (CLI, config, tray) are implemented. See [`ROADMAP.md`](ROADMAP.md), [`SPEC.md`](SPEC.md), and [`TECH_STACK.md`](TECH_STACK.md).

## Requirements

- Rust **stable** via [rustup](https://rustup.rs/) (`rustfmt`, `clippy`)
- Linux with **X11** (`DISPLAY`) and **UPower** (or sysfs fallback) for v1
- **System tray** (optional): `pkg-config`, `libgtk-3-dev`, and `libayatana-appindicator3-dev` (or `libappindicator3-dev`). `libxdo-dev` is not required for this app's tray menu.

## Project layout

```
crates/
  core/              Domain types, port traits, orchestrator (no OS deps)
  platform-linux/    Linux power + overlay adapters
  platform-windows/  Future stub
  platform-macos/    Future stub
  app/               Binary, CLI, tray, wiring
assets/shaders/      WGSL shader assets
config/              Reference TOML (`default.toml`)
scripts/             Local test and lint runners
```

## Configuration

Optional user config (merged over SPEC defaults):

1. `POWER_SHIMMER_CONFIG` — absolute path to a TOML file
2. `$XDG_CONFIG_HOME/power-shimmer/config.toml`
3. `~/.config/power-shimmer/config.toml`

See [`config/default.toml`](config/default.toml) for the full schema.

## Usage

```bash
# Background daemon with system tray (default)
cargo run -p power-shimmer-app

# Headless daemon (power loop only)
cargo run -p power-shimmer-app -- --no-tray

# One-shot manual shimmer (no tray, no power loop)
cargo run -p power-shimmer-app -- --trigger

# Log triggers without overlay
cargo run -p power-shimmer-app -- --dry-run

# Override visual parameters
cargo run -p power-shimmer-app -- --duration-ms 3000 --opacity 0.5
```

Installed binary name: `power-shimmer` (from package `power-shimmer-app`).

## Development

```bash
# Format, clippy, build, and test (canonical local gate)
./scripts/check.sh

# Tests only
./scripts/test.sh

# App unit tests without GTK/tray (CI-friendly)
cargo test -p power-shimmer-app --no-default-features --features linux

# Manual overlay (X11 DISPLAY + GPU)
cargo test -p power-shimmer-platform-linux overlay_x11 -- --ignored --nocapture

# Manual power (hardware / UPower)
cargo test -p power-shimmer-platform-linux linux_power_smoke -- --ignored --nocapture
```

## Building

```bash
cargo build -p power-shimmer-app
cargo run -p power-shimmer-app
```

Build without tray support (no GTK): `cargo build -p power-shimmer-app --no-default-features --features linux`

## License

MIT OR Apache-2.0
