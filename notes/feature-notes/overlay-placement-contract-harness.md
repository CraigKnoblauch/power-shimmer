# Overlay placement contract test harness

> **Status:** Implemented (Phase 1 + window-only probe).  
> **Crate:** [`crates/overlay-contract`](../../crates/overlay-contract) (`power-shimmer-overlay-contract`)  
> **Related:** [SPEC.md](../../SPEC.md) Module 3, [overlay-contract README](../../crates/overlay-contract/README.md)

---

## Resolved decisions (approved)

| # | Decision | Choice |
|---|----------|--------|
| 1 | Crate name | `overlay-contract` / package `power-shimmer-overlay-contract` |
| 2 | Probe trait location | Only in `overlay-contract` (`OverlayPlacementProbe`, `OverlayWindowPlacementProbe`) |
| 3 | Re-exports | `platform-linux` re-exports `window_covers_monitor` from `overlay::placement` |
| 4 | Runtime placement check | Fail `play` with `WindowCreationFailed` if window does not cover monitor (in `prepare_overlay_window`) |
| 5 | CI | PR gate: Xvfb window contract (no GPU); optional full contract with Mesa llvmpipe |

---

## What was implemented

### Crate: `overlay-contract`

| Module | Purpose |
|--------|---------|
| [`placement.rs`](../../crates/overlay-contract/src/placement.rs) | Pure geometry + monitor policy; 9 unit tests |
| [`probe.rs`](../../crates/overlay-contract/src/probe.rs) | `OverlayPlacementProbe` (full `play` contract) |
| [`window_probe.rs`](../../crates/overlay-contract/src/window_probe.rs) | `OverlayWindowPlacementProbe`, `run_primary_window_placement_contract` |
| [`integration.rs`](../../crates/overlay-contract/src/integration.rs) | `run_primary_fullscreen_contract` (GPU path) |

### Linux: shared window setup

| File | Purpose |
|------|---------|
| [`window_placement.rs`](../../crates/platform-linux/src/overlay/window_placement.rs) | `prepare_overlay_window`, `create_overlay_window`, `probe_primary_window_placement`, `LinuxWindowPlacementProbe`; X11 session guard moved here |
| [`render_loop.rs`](../../crates/platform-linux/src/overlay/render_loop.rs) | `start_session` calls `prepare_overlay_window` then wgpu init |
| [`placement.rs`](../../crates/platform-linux/src/overlay/placement.rs) | Re-exports harness symbols |

**Window probe behavior:** After creating a borderless-fullscreen window, the probe event loop polls (`about_to_wait` / `Resized`) until `inner_size` matches the monitor within tolerance, or times out after 50 attempts. This avoids false failures when the WM applies fullscreen geometry asynchronously.

### Integration tests

| Test | GPU | CI |
|------|-----|-----|
| [`overlay_window_placement_contract.rs`](../../crates/platform-linux/tests/overlay_window_placement_contract.rs) | **No** | Xvfb step, `--ignored` |
| [`overlay_placement_contract.rs`](../../crates/platform-linux/tests/overlay_placement_contract.rs) | Yes (llvmpipe ok) | Xvfb + `LIBGL_ALWAYS_SOFTWARE=1`, `--ignored` |

### CI (`.github/workflows/ci.yml`)

1. Main job: `cargo test --workspace` (both contract tests skipped via `#[ignore]`).
2. **Overlay window placement contract (Xvfb, PR gate, no GPU):** `xvfb` only, `DISPLAY=:99`, no Mesa.
3. **Overlay full placement contract:** installs `libgl1-mesa-dri`, runs GPU contract with `--ignored`.

---

## How to run locally

```bash
# Pure policy (no display, no GPU):
cargo test -p power-shimmer-overlay-contract

# Window placement (winit only):
xvfb-run -a cargo test -p power-shimmer-platform-linux overlay_window_placement_contract -- --ignored --nocapture

# Full play + GPU:
xvfb-run -a cargo test -p power-shimmer-platform-linux overlay_placement_contract -- --ignored --nocapture
```

---

## Proof matrix

| Layer | GPU | Display | Proves |
|-------|-----|---------|--------|
| `overlay-contract` units | No | No | Size policy + primary index selection |
| Window placement contract | No | Xvfb/X11 | Real borderless fullscreen geometry |
| Full placement contract | Yes | Xvfb/X11 | End-to-end `play` + snapshot |
| Runtime `prepare_overlay_window` | N/A | User machine | Fail before GPU if sizes mismatch immediately |

---

## Per-platform checklist (future)

1. Use `prepare_overlay_window` / harness placement helpers in adapter code.
2. `impl OverlayWindowPlacementProbe` + `impl OverlayPlacementProbe` on renderer.
3. `tests/overlay_window_placement_contract.rs` and `tests/overlay_placement_contract.rs`.
4. Platform `require_overlay_session` in probe impls.

---

## Evolution (not yet done)

- Windows / macOS window probes  
- Wayland layer-shell probe  
- `FakePlacementProbe` for stub adapters  
- Multi-monitor contract runners  

---

## Summary

Fullscreen placement is enforced at three levels: **pure units** (always CI), **window-only contract** (Xvfb, no GPU, PR gate), and **full GPU contract** (Xvfb + Mesa, secondary). Production code shares `prepare_overlay_window` with the probe so window attributes and monitor selection stay aligned.
