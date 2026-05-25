# power-shimmer-overlay-contract

Shared types and test harness for the SPEC overlay visual contract (primary monitor, full bounds).

## Integration tests (Linux)

```bash
# Window placement only (winit, no GPU) — needs DISPLAY or Xvfb:
xvfb-run -a cargo test -p power-shimmer-platform-linux overlay_window_placement_contract -- --ignored --nocapture

# Full play + GPU path:
xvfb-run -a cargo test -p power-shimmer-platform-linux overlay_placement_contract -- --ignored --nocapture
```

CI runs both under Xvfb (see `.github/workflows/ci.yml`): window contract without Mesa; full contract with `LIBGL_ALWAYS_SOFTWARE=1`.

## Adding a new platform

1. Depend on this crate (dev-dependency is enough for tests; normal dependency if using placement helpers in the adapter).
2. Record `OverlayPlacement` immediately after creating the overlay window.
3. Implement `OverlayPlacementProbe` on the renderer handle.
4. Add `tests/overlay_placement_contract.rs` calling `run_primary_fullscreen_contract`.
