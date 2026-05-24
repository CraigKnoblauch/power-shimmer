# Power Shimmer

A lightweight desktop utility that plays a rainbow shimmer overlay when your laptop transitions from battery to AC power.

**Status:** Project scaffold — core logic not yet implemented. See [`TECH_STACK.md`](TECH_STACK.md) and [`SPEC.md`](SPEC.md).

## Requirements

- Rust **1.75+** for the current scaffold (`core` + empty adapters)
- Rust **stable via [rustup](https://rustup.rs/)** (with `rustfmt` and `clippy`) before implementing platform adapters — the apt `cargo` 1.75 package cannot resolve modern `wgpu`/`winit`/`clap` releases
- Linux (X11 + UPower) for v1; Wayland support planned for v1.1

## Project layout

```
crates/
  core/              Domain types, port traits, orchestrator (no OS deps)
  platform-linux/    Linux power + overlay adapters
  platform-windows/  Future stub
  platform-macos/    Future stub
  app/               Binary, CLI, tray, wiring
assets/shaders/      WGSL shader assets
config/              Default TOML settings
scripts/             Local test and lint runners
```

## Development

```bash
# Format, clippy, build, and test
./scripts/check.sh

# Tests only
./scripts/test.sh

# Lint only (fmt + clippy + cargo-deny if installed)
./scripts/lint.sh
```

## Building

```bash
cargo build --workspace
cargo run -p power-shimmer-app
```

## License

MIT OR Apache-2.0
