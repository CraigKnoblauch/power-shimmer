# Issue: Shimmer draws in top-left quadrant after taskbar fix

**Status:** Fixed (platform-linux — `Normal` window type + post-show surface configure + `Resized`)  
**Severity:** High (overlay no longer covers primary monitor)  
**Introduced by:** Taskbar-hiding change (`overlay-taskbar-icon-during-animation` — commit `251a470` / policy + `WindowType::Notification`)  
**Affected:** Linux X11 overlay (`platform-linux` / `render_loop.rs`)

---

## Symptom

On laptops (often HiDPI), the rainbow shimmer appears only in the **top-left quarter** of the screen. The rest of the display is unchanged (transparent / desktop visible). Behavior was **full-screen coverage** before the taskbar-hiding fix.

This is distinct from the original issue (transient **taskbar icon** during animation).

---

## What changed in the taskbar fix

| Change | File | Taskbar relevance | Size impact |
|--------|------|-------------------|-------------|
| `with_x11_window_type([Notification])` | `render_loop.rs`, `overlay_hint_policy.rs` | Primary lever for hiding from taskbar | **High — likely cause** |
| WM state hints **after** `set_visible(true)` | `render_loop.rs`, `x11_click_through.rs` | Correct timing for `SKIP_TASKBAR` / `SKIP_PAGER` | Low (unless WM drops fullscreen) |
| Click-through hints **before** show only | `x11_click_through.rs` | Unchanged semantics for Shape | None expected |
| Empty window title | `render_loop.rs` | Cosmetic | None expected |
| Two `_NET_WM_STATE` ADD messages | `x11_click_through.rs` | Pager + taskbar | Low |

Git diff for `render_loop.rs` between pre-fix and taskbar fix shows **only** the rows above (no intentional change to surface sizing logic).

---

## Root cause assessment

### Primary: `_NET_WM_WINDOW_TYPE_NOTIFICATION` + early `surface.configure`

The taskbar fix set the overlay to **`WindowType::Notification`** so the window manager would not list it in the taskbar (see `overlay_hint_policy.rs` and EWMH guidance that notifications are “typically” small, override-redirect bubbles).

**EWMH intent:** `_NET_WM_WINDOW_TYPE_NOTIFICATION` is for short informational bubbles (e.g. “running out of power”), not full-screen presentation surfaces. Window managers use `WINDOW_TYPE` for **decoration, stacking, and geometry policy**. Many desktops (GNOME Shell, KDE Plasma, etc.) **do not treat notification-typed clients like normal fullscreen apps**, even when the client also sets borderless fullscreen and `_NET_WM_STATE_FULLSCREEN`.

**winit sizing path (unchanged before/after fix, but now interacts badly with WM):**

1. Window is created with `with_visible(false)` and `with_fullscreen(Borderless(monitor))`.
2. winit’s X11 backend defaults physical size to **800×600** when no `inner_size` is set (`winit` `platform_impl/linux/x11/window.rs` — `or_else(|| Some((800, 600).into()))`).
3. **Deferred fullscreen:** if the window is not visible yet, fullscreen is stored in `desired_fullscreen` and applied on visibility (`visibility_notify` → `set_fullscreen`).
4. **`surface.configure` runs at line ~184 using `window.inner_size()` before `set_visible(true)`.**
5. `set_visible(true)` runs later; fullscreen may expand the X11 window, but the **wgpu surface can remain configured at the pre-map size** if:
   - the WM never resizes a notification window to monitor bounds, or
   - `inner_size()` does not update (no effective resize from the client’s perspective).

The fragment shader draws a **full-screen triangle in NDC** (`shimmer.wgsl`), so the GPU always fills the **surface buffer**. If the buffer is ~half the monitor width and height, the effect is exactly a **top-left quadrant** on the physical display (common on **2× scale** laptops: logical size ≈ half physical monitor dimensions, e.g. 960×540 on a 1920×1080 panel).

**Why this appeared as a *new* bug after the taskbar fix:**  
Before the fix, the window type was winit’s default **`Normal`**. Normal + borderless fullscreen is the path WMs test and support; deferred fullscreen on show usually results in monitor-sized geometry, and `render_frame`’s per-frame `inner_size()` check often picks up the final size quickly. **`Notification` changes WM policy** so the same winit sequence leaves a small client area (or a small reported `inner_size`) while the compositor may still place the window “fullscreen” in spirit — producing the quadrant artifact.

### Secondary: configure-before-show race (pre-existing, now exposed)

Even without `Notification`, configuring the wgpu surface **before** `set_visible(true)` and **before** deferred fullscreen is applied is fragile. The taskbar fix did not add this race; it **surfaced** it by combining:

- small/default geometry friendly to notification windows, and
- no `WindowEvent::Resized` handler in `OverlayApp::window_event` (only `RedrawRequested` / `CloseRequested`).

`render_frame` does reconfigure when `inner_size()` changes, but if the WM keeps a small buffer for notification clients, every frame still sees a small size → **no recovery**.

### Unlikely causes

| Hypothesis | Verdict |
|------------|---------|
| Empty title | No mechanism to change geometry |
| `SKIP_TASKBAR` / `SKIP_PAGER` after show | Might affect taskbar listing; only affects size if WM clears `FULLSCREEN` (possible on some WMs — worth checking via `xprop` during repro) |
| Split click-through vs WM-state hints | Shape extension does not change window dimensions |
| HiDPI alone | Would have existed on pre-fix laptops if size stayed wrong; user reports regression tied to taskbar change |

---

## Confirmation steps (manual)

During `--trigger` shimmer on the affected laptop:

```bash
# Replace WIN with overlay X11 id from xwininfo -tree -root | grep -i shimmer
xprop -id WIN _NET_WM_WINDOW_TYPE _NET_WM_STATE WM_NORMAL_HINTS
xwininfo -id WIN | grep -E 'geometry|Width|Height'
```

| Observation | Interpretation |
|-------------|----------------|
| `_NET_WM_WINDOW_TYPE_NOTIFICATION` | Matches our policy change |
| Geometry much smaller than monitor | WM not fullscreen-sizing notification clients |
| `inner_size` in logs (add temporary `tracing::debug!`) ≈ quarter of monitor | Confirms wgpu buffer mismatch |

Compare with reverting only window type to `Normal` (keep post-show `SKIP_TASKBAR` / `SKIP_PAGER`): if full-screen returns, **Notification** is confirmed as the regression source.

---

## Planned fix direction (not implemented)

Stay within **`platform-linux`**; keep SPEC “hidden from taskbar (best effort)” without requiring `Notification`.

### Recommended approach (ordered)

1. **Revert `WindowType::Notification`** for the overlay. Use **`Normal`** (or evaluate **`Splash`** if taskbar tests pass on target WMs — startup splash is closer to full-screen overlay semantics than notification bubbles).

2. **Keep** post-show **`SKIP_TASKBAR` + `SKIP_PAGER`** and pre-show click-through Shape hints — those address the original taskbar icon issue without changing window type.

3. **Harden sizing** (defensive, works for all window types):
   - After `set_visible(true)` (and after WM-state hints), **reconfigure the surface** using `monitor.size()` and/or `window.inner_size()`; optionally `window.request_inner_size(monitor.size())` if needed.
   - Handle **`WindowEvent::Resized`** and reconfigure once per size change.
   - Add a debug log of `(inner_size, monitor.size())` at configure time for Phase 5 laptops.

4. **Policy / tests:** Update `overlay_hint_policy::overlay_x11_window_types()` and unit tests to match the chosen type; add a regression test or comment that **`Notification` must not be used** for full-screen overlays (documented here).

### SPEC / architecture

- No `core` trait change.
- Taskbar hiding remains platform adapter detail.
- Optional SPEC annex later: “Linux: prefer `_NET_WM_STATE_SKIP_TASKBAR` on `Normal` fullscreen overlay; do not use `NOTIFICATION` for full-monitor coverage.”

---

## Relationship to other notes

| Document | Link |
|----------|------|
| Original taskbar issue | `overlay-taskbar-icon-during-animation.md` (fix landed; this doc tracks regression) |
| SPEC visual row | Taskbar not listed; full primary monitor bounds unchanged |

---

## Suggested verification after fix

| Check | Expected |
|-------|----------|
| Laptop `--trigger` | Full-monitor shimmer |
| Laptop taskbar during play | No transient overlay icon (original goal) |
| `cargo test -p power-shimmer-platform-linux` | Policy tests updated; green |
| Non-HiDPI / external monitor | Still full-screen |
