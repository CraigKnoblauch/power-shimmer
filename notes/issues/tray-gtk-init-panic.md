# Issue: GTK tray initialization panic kills daemon mode

**Status:** Identified — not yet fixed  
**Severity:** High (default entry path aborts on startup)  
**Affected entry path:** Daemon with tray (default)  
**Not affected:** `--trigger`, `--no-tray`

---

## Identification

### Symptom

Running the full program (`power-shimmer`, default daemon + tray) on a laptop appears to stop when AC power is connected. Manual shimmer via `--trigger` works. Plugging in the charger does not produce the expected Battery → AC shimmer.

### Observed log sequence

1. Daemon starts; overlay and orchestrator initialize successfully.
2. UPower falls back to sysfs (separate issue — see `upower-online-property.md`).
3. Main thread panics during tray setup:

   ```
   thread 'main' panicked at .../gtk-0.18.2/src/auto/menu.rs:29:9:
   GTK has not been initialized. Call `gtk::init` first.
   ```

4. Several seconds later, sysfs logs AC online change (`last=Some(false) current=Some(true)`), but the process has already aborted.

### Root cause

`app/tray.rs` creates a GTK-backed menu via `tray_icon::menu::Menu::new()` without calling `gtk::init()` first, and without running a GTK event loop on the tray thread.

Relevant code path:

```
run_daemon() → run_tray_or_error() → run_tray() → build_menu() → Menu::new()  [panic]
```

The `tray-icon` crate (Linux) requires:

1. `gtk::init()` on the same thread that creates the tray icon and menu.
2. A GTK event loop on that thread (`gtk::main()` or periodic `gtk::main_iteration()`).

Current implementation uses a `thread::sleep(16)` polling loop for menu events, which does not satisfy either requirement.

### Why it correlates with plugging in the charger

The panic occurs at startup (~1 s after launch), not at plug-in time. The user typically plugs in shortly after starting the daemon. The independent sysfs poll thread continues logging briefly after the main thread panics, making it look as though plug-in caused the failure. Power detection itself works; the orchestrator never survives long enough to handle the transition.

### Confirmation test

Run `power-shimmer --no-tray` on battery, then plug in AC. If shimmer plays, the power pipeline and orchestrator are healthy and the tray is the sole blocker for default mode.

---

## Planned resolution

All changes confined to the `app` crate (composition root). No changes to `core` ports, orchestrator policy, or platform adapters.

### 1. GTK initialization

Before any `Menu::new()` or `TrayIconBuilder::build()` call on Linux, call `gtk::init()` on the tray thread.

### 2. Dedicated tray thread with GTK event loop

Move tray creation and event handling to a dedicated OS thread (recommended by `tray-icon` documentation):

- Thread startup: `gtk::init()` → build menu → build tray icon.
- Event loop: pump GTK events (`gtk::events_pending()` / `gtk::main_iteration()`) while also draining `MenuEvent::receiver()`.
- Shutdown: signal quit from tray "Quit" menu item; exit loop cleanly and join thread.

Keep orchestrator interaction via existing `Arc<LinuxOrchestrator>` + `tokio::runtime::Handle::current().spawn(...)` for async calls (`trigger_manual`, `set_auto_enabled`, `shutdown`).

### 3. Graceful failure instead of panic

Convert GTK init / tray build failures into `AppError::Tray` returns so `run_daemon` exits with a non-zero code and a log message rather than aborting via panic.

### 4. Dependencies

Add explicit `gtk` crate dependency to `app` (Linux + tray feature only) for `gtk::init()` and event pumping. Keep `tray-icon` with `default-features = false` per existing ROADMAP decision (no `libxdo`).

### Verification

| Test | Expected |
|------|----------|
| Default daemon on battery → plug AC | Shimmer plays; tray remains active |
| Tray: Play now | Manual shimmer via `trigger_manual()` |
| Tray: Enable/Disable auto | `set_auto_enabled()` toggles |
| Tray: Quit | Clean `shutdown()` |
| `--trigger` | Unchanged |
| `--no-tray` | Unchanged |
| `cargo test -p power-shimmer-core` | No regressions |

---

## SPEC.md compliance

### Does this issue violate SPEC?

**No.** The bug is an implementation defect in app-layer tray wiring. SPEC defines tray behavior and orchestrator calls; it does not require GTK initialization details. The intended architecture (tray → orchestrator, power listener → orchestrator → overlay) was never reached because the process crashed first.

### Does the planned fix violate SPEC?

**No.** The fix stays within approved boundaries:

| SPEC rule | Fix alignment |
|-----------|---------------|
| Tray menu actions call orchestrator (`Play now` → `trigger_manual()`, etc.) | Preserved — only the GTK lifecycle around those calls changes |
| `--trigger` and daemon mode are mutually exclusive entry paths | Unchanged |
| Power Listener never calls Overlay Renderer | Unchanged — tray fix does not touch power or overlay ports |
| Orchestrator owns Battery → AC policy | Unchanged |
| `core` has zero platform/GUI dependencies | Unchanged — GTK stays in `app` only |
| Adapters translate, not decide | Unchanged |

### SPEC sections touched (behaviorally, not structurally)

- **App-Layer Integration — Tray menu actions (v1):** Implementation must satisfy these calls; the fix enables them rather than altering the contract.
- **CLI flags — `--no-tray`:** Remains valid headless fallback; no spec change needed.

No revision to SPEC.md is required for this fix.
