# Power Shimmer — v1 Roadmap

> **Authoritative contracts:** [`SPEC.md`](SPEC.md) · **Stack:** [`TECH_STACK.md`](TECH_STACK.md)  
> **Goal:** Ship a usable Linux daemon that plays a primary-monitor rainbow shimmer on **Battery → AC** only, controlled via **system tray** and **CLI**.

---

## v1 Definition of Done

A laptop on **Linux (X11 + UPower)** can run `power-shimmer` as a background utility that:

1. Subscribes to real power state (UPower, with sysfs fallback when D-Bus is unavailable).
2. On plug-in (**Battery → AC**), plays a ~2s full-screen GPU shimmer on the **primary monitor** (click-through, no focus).
3. Exposes **tray** actions: Play now, Enable/Disable auto shimmer, Quit.
4. Exposes **CLI** flags from SPEC (daemon, `--trigger`, `--dry-run`, `--no-tray`, duration/opacity overrides).
5. Loads optional **TOML** config; respects orchestrator policy (`auto_enabled`, `dry_run`, overlap **Skip** by default).
6. Passes **unit tests** on mock ports and a **Linux integration smoke test** with UPower.

**Explicitly out of v1 scope** (see SPEC “Version Roadmap Hooks”): Wayland overlay (v1.1), Windows/macOS adapters, multi-monitor, hotkeys.

---

## Current Progress (baseline)

| Area | Status | Notes |
|------|--------|--------|
| Workspace & crates | Done | `core`, `platform-linux`, `app`, stubs for win/macOS |
| Domain types | Done | `PowerSource`, `PowerEvent`, `ShimmerConfig`, `OrchestratorConfig`, `OverlapPolicy`, errors in `core::domain` |
| `PowerEventListener` port | Done | `subscribe()` → `PowerEventStream` (`recv` / `recv_timeout` → `StreamRecvResult`) |
| `OverlayRenderer` port | Done | Trait + SPEC docs in `crates/core/src/ports/overlay.rs`; no platform deps |
| `ShimmerOrchestrator` | Done | `services/shimmer_orchestrator.rs`; `run`, `trigger_manual`, `shutdown`, overlap/dry-run policy |
| `should_auto_play` policy | Done | `services/policy.rs` pure predicate + unit tests |
| `OrchestratorConfig` / overlap policy | Done | `domain/config.rs`; defaults: `Skip`, `auto_enabled: true`, `dry_run: false` |
| Generic Linux power listener | Done | `LinuxPowerListener` + `PowerSourceBackend`; trailing-edge 400 ms debounce; emits `InitialState` + `Transition` |
| UPower / sysfs backends | Done | `UpowerBackend` (D-Bus `OnLine` + `PropertiesChanged`); `SysfsFallbackBackend` (sysfs poll); `LinuxPowerBackend::select()` |
| 400 ms debounce coalescing | Done | Trailing-edge coalesce in `listener.rs`; unit test `debounce_coalesces_rapid_flicker_into_single_transition` |
| `MockPowerEventListener` (core) | Done | `core::testing::mock_power.rs`; injects `Vec<PowerEvent>` |
| `MockOverlayRenderer` (core) | Done | `core::testing::mock_overlay.rs`; `play_calls`, delay, `cancel()` → `Cancelled` |
| Overlay (X11 + wgpu) | Done | `WgpuShimmerRenderer`; `wgpu_shimmer.rs`, `render_loop.rs`, `session.rs`, `shader.rs`, `x11_click_through.rs`; winit **X11-only**; `assets/shaders/shimmer.wgsl` |
| Overlay adapter tests | Done | Session + shader unit tests; `cancel_when_idle`; GPU/X11 tests `#[ignore]` in `wgpu_shimmer.rs` |
| Overlay X11 smoke test | Wired, ignored | `tests/overlay_x11_smoke.rs`; run with `--ignored` on X11 + GPU |
| App (CLI, tray, wiring, config) | Not started | `main` prints placeholder message |
| Orchestrator unit tests (SPEC table) | Done | All 8 SPEC rows + shutdown in `orchestrator_test.rs` |
| Mock overlay unit tests (SPEC) | Done | `is_playing`, `Cancelled`, completion covered in `mock_overlay.rs` unit tests |
| Linux power smoke test | Wired, ignored | `linux_power_smoke.rs` uses `LinuxPowerBackend::select()`; run with `--ignored` on hardware |

**Reference implementation today:** `cargo test -p power-shimmer-core` — **20 tests green** (policy, mocks, full orchestrator SPEC table). `cargo test -p power-shimmer-platform-linux` — **16 tests green**, 3 ignored (2 GPU/X11 unit tests + overlay smoke + power smoke). Phases **1–3 complete** (overlay not yet wired in app). Production paths: power `LinuxPowerBackend::select()` → `LinuxPowerListener`; overlay `WgpuShimmerRenderer` (X11 + wgpu, Wayland deferred v1.1).

---

## Recommended Build Order

Work top-down in **test-first** slices. Each phase should end with `cargo test` / `./scripts/check.sh` green before the next.

```mermaid
flowchart TD
    subgraph done [Done]
        A[Domain + power port]
        B[LinuxPowerListener + mock backend test]
        C[OrchestratorConfig domain]
        D[Mock ports]
        I[OverlayRenderer port]
        E[ShimmerOrchestrator + SPEC unit tests]
        F[UPower PowerSourceBackend]
        G[sysfs fallback]
        H[Debounce + adapter tests]
        J[X11 window + click-through]
        K[wgpu shader + play/cancel]
    end
    subgraph p4 [Phase 4 — Ship]
        L[Config TOML]
        M[App wiring + CLI]
        N[Tray menu]
        O[Smoke test + polish]
    end
    A --> B --> C --> D --> E
    E --> F --> G --> H
    E --> I --> J --> K
    H --> L
    K --> L
    L --> M --> N --> O
```

Phases 2 and 3 can proceed in parallel once Phase 1 lands; Phase 4 requires both. **Phases 2 and 3 are complete** — proceed with Phase 4 (app wiring, CLI, tray, end-to-end `--trigger`).

---

## Phase 1 — Core orchestration (mock ports only)

**Objective:** Battery → AC policy and manual triggers work entirely in `core` with no OS or GPU code.

| # | Task | SPEC / files | Acceptance | Status |
|---|------|----------------|------------|--------|
| 1.1 | Add `OrchestratorConfig`, `OverlapPolicy`, defaults | `domain/config.rs` | Compiles; defaults match SPEC (`Skip`, `auto_enabled: true`, etc.) | **Done** |
| 1.2 | Implement `OverlayRenderer` trait in `ports/overlay.rs` | Module 3 | Trait + docs; no platform deps | **Done** |
| 1.3 | `MockPowerEventListener` | `testing/mock_power.rs` | Injects `Vec<PowerEvent>` for tests | **Done** |
| 1.4 | `MockOverlayRenderer` | `testing/mock_overlay.rs` | Records `play_calls`; supports `delay` + `cancel()` → `Cancelled` | **Done** |
| 1.5 | Implement `ShimmerOrchestrator` | `services/shimmer_orchestrator.rs` | `new`, `run`, `trigger_manual`, `update_config`, `set_auto_enabled`, `shutdown` | **Done** |
| 1.6 | Orchestrator unit tests | SPEC “Unit Test Requirements” | All rows in orchestrator table pass | **Done** |
| 1.7 | Optional: `should_auto_play` as pure fn + tests | `services/policy.rs` | Documents policy without integration | **Done** |

**Exit criteria:** `cargo test -p power-shimmer-core` covers orchestrator behavior; zero `platform-linux` / `winit` / `zbus` in `core`.

---

## Phase 2 — Linux power adapters (real OS)

**Objective:** Replace the test-only `MockPowerBackend` with production backends while keeping `LinuxPowerListener` unchanged.

| # | Task | SPEC / files | Acceptance | Status |
|---|------|----------------|------------|--------|
| 2.1 | `UpowerBackend` implements `PowerSourceBackend` | `power/upower.rs`, `zbus` | Reads `OnLine`; maps to `PowerSource` | **Done** |
| 2.2 | Wire `LinuxPowerListener::new(UpowerBackend::…)` | App wiring (Phase 4) or integration test | `InitialState` reflects live state in integration | **Partial** — smoke test wired; app composition root in Phase 4 |
| 2.3 | `SysfsFallbackBackend` | `power/sysfs_fallback.rs` | Same event protocol; selected when UPower unavailable | **Done** |
| 2.4 | Backend selection | `power/mod.rs` (`LinuxPowerBackend::select`) | UPower preferred; sysfs when D-Bus unavailable | **Done** |
| 2.5 | Debounce coalescing (400 ms) | `power/listener.rs` | Two rapid `online` toggles within 400 ms → **one** `Transition` | **Done** |
| 2.6 | `InitialState { Unknown }` + background retry | `power/upower.rs` | Does not block orchestrator when UPower down at startup | **Done** |
| 2.7 | Unit test: debounce coalescing | `power/listener.rs` tests | Mock backend pushes flicker; assert single transition | **Done** |
| 2.8 | Enable `linux_power_smoke` (ignored → run on CI/hardware) | `tests/linux_power_smoke.rs` | Manual/CI: plug AC → receive `Transition(Battery, Ac)` | **Partial** — test wired; still `#[ignore]` until CI/hardware run |

**Exit criteria:** Daemon can subscribe to real power on a Linux laptop; listener still does **not** decide shimmer policy. **Met** (pending Phase 4 daemon wiring and manual smoke run).

---

## Phase 3 — Linux overlay (X11 + wgpu)

**Objective:** Satisfy `OverlayRenderer` for primary monitor on X11. **Wayland deferred to v1.1.**

| # | Task | SPEC / files | Acceptance | Status |
|---|------|----------------|------------|--------|
| 3.1 | Add workspace deps to `platform-linux` | `Cargo.toml` | `tokio`, `winit`, `wgpu`, `x11rb`, `bytemuck`, `pollster`, `raw-window-handle` | **Done** |
| 3.2 | X11 click-through + window hints | `overlay/x11_click_through.rs` | No focus; pass-through input; hidden from taskbar (best effort) | **Done** |
| 3.3 | Primary monitor bounds | `overlay/render_loop.rs` | Full primary display coverage (borderless fullscreen) | **Done** |
| 3.4 | `WgpuShimmerRenderer` implements `OverlayRenderer` | `overlay/wgpu_shimmer.rs` | `play` / `is_playing` / `cancel` | **Done** |
| 3.5 | WGSL rainbow + shimmer band | `assets/shaders/shimmer.wgsl` | Honors `duration_ms`, `opacity`, `speed` ± one frame | **Done** |
| 3.6 | Teardown ≤ 500 ms | `overlay/session.rs`, render loop | Resources released after complete or cancel | **Done** |
| 3.7 | Mock overlay unit tests in `core` or `platform-linux` | SPEC overlay tests | `is_playing`, `Cancelled`, completion | **Done** (core `mock_overlay.rs` + adapter session/shader tests) |
| 3.8 | Manual test: `power-shimmer --trigger` | App (Phase 4) | Visible shimmer without power event | **Pending** (Phase 4) |

**Exit criteria:** Orchestrator + real overlay completes end-to-end on X11; `wayland_layer_shell.rs` remains stubbed. **Met** for adapter crate (pending Phase 4 composition root and manual `--trigger` / AC plug E2E).

---

## Phase 4 — Application shell & release polish

**Objective:** User-facing binary wires everything together.

| # | Task | SPEC / files | Acceptance |
|---|------|----------------|------------|
| 4.1 | TOML config load + defaults | `app/config.rs`, `config/` | `ShimmerConfig` + orchestrator flags |
| 4.2 | `clap` CLI | SPEC CLI table | `--trigger`, `--dry-run`, `--no-tray`, `--duration-ms`, `--opacity` |
| 4.3 | Composition root | `app/wiring.rs` | Builds UPower listener, X11 overlay, orchestrator |
| 4.4 | Tokio runtime + task spawn | TECH_STACK | Power loop in background; overlay on orchestrator tasks |
| 4.5 | Entry paths | SPEC | `--trigger` → manual play + exit; default → daemon |
| 4.6 | System tray | `app/tray.rs`, `tray-icon` | Play now / Enable auto / Quit → orchestrator calls |
| 4.7 | Logging | `tracing` | Dry-run and trigger paths log intent |
| 4.8 | `scripts/check.sh` in CI / docs | README | Format, clippy, test, deny (if configured) |
| 4.9 | Update README status | — | Reflects v1 shipped capabilities |

**Exit criteria:** `cargo run -p power-shimmer-app` (or project binary name) runs daemon on X11; unplug/replug AC triggers shimmer when auto is enabled.

---

## Phase 5 — v1 hardening (before tag)

| # | Task | Notes |
|---|------|--------|
| 5.1 | End-to-end manual test matrix | Boot on AC, boot on battery + plug, unplug (no shimmer), manual play on battery |
| 5.2 | Error paths | Stream ended, overlay failure surfaced in logs without panic |
| 5.3 | Resource leak check | Repeated plug/unplug; memory stable |
| 5.4 | Packaging notes | `.desktop`, install path — optional for first tag |
| 5.5 | SPEC / README sync | Mark implemented modules; link `ROADMAP.md` |

---

## Post-v1 (tracked, not scheduled here)

| Version | Feature | Impact |
|---------|---------|--------|
| **v1.1** | Wayland `layer-shell` overlay | New `OverlayRenderer` impl; `wayland` feature flag |
| **Future** | Windows / macOS | New platform crates; `core` unchanged |
| **Future** | Multi-monitor | Extend `MonitorTarget`; config + orchestrator pass-through |

---

## Quick reference — SPEC modules → crates

| SPEC module | Crate | Path | Roadmap phase |
|-------------|-------|------|----------------|
| Domain types | `core` | `domain/` | Done |
| Power Listener port | `core` | `ports/power.rs` | Done |
| Power Listener adapter | `platform-linux` | `power/listener.rs`, `upower.rs`, `sysfs_fallback.rs`, `mod.rs` | Done |
| Overlay Renderer port | `core` | `ports/overlay.rs` | Done |
| Overlay Renderer adapter | `platform-linux` | `overlay/wgpu_shimmer.rs`, `render_loop.rs`, `session.rs`, `shader.rs`, `x11_click_through.rs` | Done |
| Shimmer Orchestrator | `core` | `services/shimmer_orchestrator.rs`, `services/policy.rs` | Done |
| Test doubles | `core` | `testing/mock_power.rs`, `testing/mock_overlay.rs` | Done |
| CLI / tray / wiring | `app` | `main.rs`, `wiring.rs`, `tray.rs`, `config.rs` | 4 |

---

## Suggested next step

**Phases 2 and 3 are complete.** Next slice:

- **Phase 4:** App wiring — `LinuxPowerBackend::select()` + `WgpuShimmerRenderer` + orchestrator daemon loop, CLI (`--trigger`), tray

```bash
cargo test -p power-shimmer-core
cargo test -p power-shimmer-platform-linux
# Manual overlay (X11 DISPLAY + GPU):
cargo test -p power-shimmer-platform-linux overlay_x11 -- --ignored --nocapture
# Manual power (hardware / UPower):
cargo test -p power-shimmer-platform-linux linux_power_smoke -- --ignored --nocapture
./scripts/check.sh
```
