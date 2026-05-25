# Issue: Taskbar icon appears during overlay animation

**Status:** Fixed (platform-linux X11 overlay)  
**Severity:** Medium (visual/UX regression; does not break shimmer playback)  
**Affected entry path:** Any path that plays the overlay (`--trigger`, daemon auto/manual shimmer)  
**Not affected:** System tray icon (intentional, separate surface)

---

## Identification

### Symptom

While the rainbow shimmer animation runs (~2s by default), a **second** application icon appears in the desktop taskbar (or dock). It disappears when the overlay window is hidden/destroyed after the animation completes. The persistent **tray** icon from daemon mode is expected; this transient icon is not.

### Expected behavior (SPEC)

[`SPEC.md`](../../SPEC.md) Module 3 — Overlay Renderer:

| Property | Requirement |
|----------|-------------|
| Taskbar | **Not listed** (platform best-effort) |

Port documentation in `core::ports::overlay` repeats: *hidden from taskbar (best effort)*.

[`ROADMAP.md`](../../ROADMAP.md) Phase 3.2 marks X11 click-through + taskbar hiding as **Done**; manual Phase 5 E2E validation is still pending, which is how this regression surfaced.

### Root cause

The overlay is a **transient winit X11 window** (`render_loop.rs`) titled `"Power Shimmer"`, borderless, fullscreen on the primary monitor. Taskbar integration is controlled by EWMH hints in `x11_click_through.rs`, but the current wiring does not reliably keep the window off the taskbar on common desktops (GNOME, KDE, XFCE).

Contributing factors (combined):

1. **Hint timing** — `_NET_WM_STATE_SKIP_TASKBAR` is applied once immediately after `create_window`, **before** `set_visible(true)` and before the window manager fully maps a borderless fullscreen client. Many WMs assign taskbar entries at map time; a pre-map `ClientMessage` may be ignored or superseded when fullscreen/`_NET_WM_STATE_FULLSCREEN` is applied.

2. **Default window type `Normal`** — winit creates `_NET_WM_WINDOW_TYPE_NORMAL` unless overridden. Normal top-level windows are exactly what taskbars/docks enumerate. `SKIP_TASKBAR` should still work, but several environments treat fullscreen `Normal` windows inconsistently unless the window type is a non-taskbar role (e.g. `NOTIFICATION`, `SPLASH`, `TOOLTIP`).

3. **winit has no X11 `skip_taskbar` API** — Unlike Windows (`WindowAttributesExtWindows::with_skip_taskbar`), winit 0.30’s X11 backend never sets `_NET_WM_STATE_SKIP_TASKBAR` (the atom is not even defined in winit’s X11 atom table). Hiding from the taskbar is entirely our `x11rb` responsibility.

4. **Best-effort error swallowing** — `apply_x11_overlay_hints_best_effort` logs a warning and continues if hints fail (wrong backend, XWayland quirks, protocol errors). Playback still works; the user sees a taskbar entry with no obvious failure.

5. **Secondary X11 connection** — Hints use a fresh `RustConnection` while the window is owned by winit’s display connection. This is valid for the same `XID`, but increases the chance of races if hints are sent before winit finishes configuring the window.

The transient icon is therefore the **overlay window** being classified as a normal mapped application window, not a bug in the tray (`app/tray.rs`) or orchestrator.

### Relevant code paths

```
WgpuShimmerRenderer::play()
  → overlay thread: OverlayApp::start_session()
       → create_window(WindowAttributes { title: "Power Shimmer", fullscreen, ... })
       → apply_x11_overlay_hints_best_effort()   // once, pre-show
       → ... wgpu setup ...
       → window.set_visible(true)                // map → WM may add taskbar entry
       → render frames ...
       → finish_session() → set_visible(false)
```

Taskbar hint implementation:

- `crates/platform-linux/src/overlay/x11_click_through.rs` — `set_skip_taskbar()` via `_NET_WM_STATE` `ClientMessage`
- `crates/platform-linux/src/overlay/render_loop.rs` — call site and window attributes

### Confirmation checks (manual)

On an X11 session (`echo $DISPLAY`, not pure Wayland-only):

```bash
# During animation (second terminal, while shimmer visible):
xprop -id $(xdotool getactivewindow) _NET_WM_STATE _NET_WM_WINDOW_TYPE WM_CLASS 2>/dev/null
# Or list windows named Power Shimmer:
wmctrl -l | grep -i shimmer
```

| Observation | Interpretation |
|-------------|----------------|
| `_NET_WM_WINDOW_TYPE_NORMAL` and no `SKIP_TASKBAR` in `_NET_WM_STATE` | Hints not applied or cleared at map |
| `SKIP_TASKBAR` present but icon still shown | WM ignores hint for `Normal` fullscreen — need window type change |
| `warn!(... "X11 overlay hints failed")` in logs | `apply_x11_overlay_hints` failed silently from user POV |

Compare with tray: daemon tray icon persists across animations; overlay icon should not appear at all during play.

---

## Planned resolution

All changes stay in **`platform-linux`** overlay adapter code (`overlay/render_loop.rs`, `overlay/x11_click_through.rs`). No `core` trait changes, no orchestrator/app wiring changes, no new dependencies in `core`.

### 1. Set non-taskbar EWMH window type at creation (winit)

In `start_session`, use `winit::platform::x11::{WindowAttributesExtX11, WindowType}`:

```rust
.with_x11_window_type(vec![WindowType::Notification])  // or Splash
```

Prefer **`Notification`** (SPEC: transient overlay; EWMH describes it as typically override-redirect and not listed in taskbar) or **`Splash`** (startup overlay semantics). Keep `platform-linux` as the only crate importing winit X11 extensions.

Rationale: window type is applied by winit during window creation on the same X connection, before map — more reliable than a late `ClientMessage` alone.

### 2. Re-apply X11 hints after the window is shown

Call `apply_x11_overlay_hints_best_effort` (or a dedicated `reapply_taskbar_hiding`) **after** `window.set_visible(true)` (and optionally once on first `RedrawRequested`) so `_NET_WM_STATE_SKIP_TASKBAR` is set when the WM has a mapped client.

Keep the pre-show call for input-shape (click-through) if needed, or split:

- **Input shape** — can remain pre-show (Shape extension on unmapped window is fine).
- **Taskbar / WM state** — post-show (and on re-show if the window is ever reused).

### 3. Strengthen taskbar hiding in `x11_click_through.rs`

Within platform-linux only:

| Addition | Purpose |
|----------|---------|
| `_NET_WM_STATE_SKIP_PAGER` | Hides from alt-tab / pager on some WMs (companion to skip taskbar) |
| `_NET_WM_STATE_SKIP_TASKBAR` (retain) | Primary EWMH taskbar exclusion |
| Optional: minimal/empty window title | Reduces identifiable clutter if a WM still shows a entry briefly |

Do **not** add `core` APIs for these atoms — implementation detail of the Linux overlay adapter.

### 4. Logging and Phase 5 verification

- Log at **debug** when hints succeed; keep **warn** on failure (already present).
- Extend Phase 5 manual matrix (ROADMAP 5.1): `--trigger` and Battery→AC shimmer — confirm **no** transient taskbar/dock entry on target DE (GNOME, KDE, XFCE minimum).

Optional ignored smoke: document `xprop` checks in `overlay_x11_smoke.rs` comments (automated taskbar assertion is flaky in CI without a running WM).

### 5. Out of scope for this fix

| Item | Reason |
|------|--------|
| Wayland native overlay | v1.1 (`wayland_layer_shell.rs`); different protocol |
| Tray icon behavior | Intentional per SPEC user controls |
| Windows `with_skip_taskbar` | Future `platform-windows` adapter |

---

## SPEC.md / ROADMAP.md compliance

### Does this issue violate SPEC?

**No.** The behavior is a **platform adapter gap** against an existing contract (“hidden from taskbar, best effort”). SPEC and `OverlayRenderer` docs already require the behavior; implementation does not consistently achieve it on Linux/X11.

### Does the planned fix violate architectural boundaries?

**No.** The fix aligns with approved boundaries:

| SPEC / architecture rule | Fix alignment |
|--------------------------|---------------|
| Overlay hints live in `platform-linux`, not `core` | All edits in `overlay/*` only |
| `core` has zero `winit` / `x11rb` deps | Unchanged |
| Orchestrator never imports platform crates | Unchanged |
| Overlay Renderer does not read power state | Unchanged |
| Power Listener never calls Overlay | Unchanged |
| App composition root only wires adapters | Unchanged |
| “Best effort” taskbar hiding | Strengthens adapter; no SPEC revision required |

### Crate / module impact

| Crate | Change |
|-------|--------|
| `platform-linux` | `render_loop.rs`, `x11_click_through.rs` |
| `core` | None |
| `app` | None |

### Tests

| Test | Expected |
|------|----------|
| `cargo test -p power-shimmer-core` | No regressions (mocks unchanged) |
| `cargo test -p power-shimmer-platform-linux` | Existing unit tests pass; optional pure-Rust tests for hint helpers if extracted |
| Manual `--trigger` on X11 | Shimmer plays; **no** transient taskbar icon |
| Daemon tray + auto shimmer | Tray remains; still no overlay taskbar icon |

---

## Suggested implementation order

1. Add `with_x11_window_type(Notification)` to window attributes.  
2. Move or duplicate skip-taskbar application to after `set_visible(true)`.  
3. Add `SKIP_PAGER` in `set_skip_taskbar` (or sibling helper).  
4. Manual verification on target DE; adjust window type if one WM misbehaves.  
5. Update ROADMAP Phase 5.1 checklist when confirmed.

No revision to [`SPEC.md`](../../SPEC.md) is required unless product owners want to name a preferred `WindowType` in the visual contract table (optional documentation only).
