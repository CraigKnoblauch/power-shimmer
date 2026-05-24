# Power Shimmer — Tech Stack Proposal

> **Status:** Approved — see [`SPEC.md`](SPEC.md) for interface contracts. Implementation not yet started.

## Executive Summary

**Power Shimmer** is a lightweight desktop utility that detects when a laptop's AC power cable is plugged in and plays a brief, full-screen "rainbow shimmer" overlay animation. The design prioritizes a small idle footprint, clean separation of concerns, and a **platform-adapter pattern** so Linux is implemented first while Windows and macOS can be added later without rewriting core logic.

---

## 1. Recommended Language & Framework

### Primary recommendation: **Rust (workspace crate layout)**

| Criterion | Why Rust fits |
|---|---|
| Cross-platform | Single codebase; platform differences isolated behind traits/adapters |
| Lightweight | Small binary, low RAM — appropriate for an always-available background utility |
| Performance | GPU shader animation stays smooth without a heavy runtime |
| Architecture | Traits map cleanly to Clean Architecture ports (Dependency Inversion) |
| Ecosystem | Mature crates for windows, D-Bus, async, and GPU rendering |

### Supporting libraries (not a "framework" in the web sense)

| Layer | Library | Role |
|---|---|---|
| Async runtime | [`tokio`](https://docs.rs/tokio) | Drives power-event subscriptions and overlay lifecycle without blocking |
| Windowing | [`winit`](https://docs.rs/winit) | Cross-platform transparent, borderless, always-on-top overlay window |
| Rendering | [`wgpu`](https://docs.rs/wgpu) | GPU fragment-shader rainbow shimmer (efficient, portable) |
| Linux power | [`zbus`](https://docs.rs/zbus) | Async D-Bus client for UPower AC plug/unplug events |
| Config | [`serde`](https://docs.rs/serde) + [`toml`](https://docs.rs/toml) | User settings (duration, intensity, enable/disable) |
| System tray | [`tray-icon`](https://docs.rs/tray-icon) | Quit, toggle, and settings entry point |

### Alternatives considered (and why they were not chosen)

| Option | Verdict |
|---|---|
| **Tauri / Electron** | UI framework overhead is unnecessary for a shader overlay + tray icon; larger memory footprint |
|I surface |
| **Python + PyQt/GTK** | Viable for Linux-only prototyping, but packaging, GPU overlay performance, and cross-platform distribution are weaker |
| **Go** | Good for system services, but the graphics/overlay ecosystem is thinner than Rust's `winit` + `wgpu` stack |
| **C++ + Qt** | Powerful, but higher boilerplate and harder to keep core logic cleanly isolated from Qt types |

### High-level runtime model

```
┌─────────────────────────────────────────────────────────┐
│  App (composition root)                                 │
│  ┌──────────────┐    events     ┌─────────────────────┐ │
│  │ Power        │──────────────▶│ Shimmer             │ │
│  │ Listener     │               │ Orchestrator (core) │ │
│  │ (platform)   │               └──────────┬──────────┘ │
│  └──────────────┘                          │ trigger    │
│                                            ▼            │
│                                 ┌─────────────────────┐ │
│                                 │ Overlay Renderer    │ │
│                                 │ (platform + wgpu)   │ │
│                                 └─────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

The **Orchestrator** lives in `core` and contains no platform or rendering imports — it only speaks in terms of traits and domain events.

---

## 2. Two Core System Components

These are the two primary **ports** (interfaces) the application depends on. Everything else (tray, config, logging) is supporting infrastructure.

### Component A — Power Event Listener

**Responsibility:** Detect transitions to *AC power connected* (and optionally *disconnected*) and emit normalized domain events.

**Interface (conceptual):**

```rust
// core port — no platform imports
trait PowerEventSource {
    fn subscribe(&self) -> impl Stream<Item = PowerEvent>;
}

enum PowerEvent {
    AcConnected,
    AcDisconnected,
    BatteryLow,   // optional future extension
}
```

**Behavior:**
- Subscribe to OS power notifications (not polling, when possible).
- Debounce/noise-filter rapid plug/unplug flicker at the adapter layer.
- Map platform-specific signals into the shared `PowerEvent` enum.
- On Linux v1: fire `AcConnected` when UPower reports `on-line == true` after being `false`.

**Single Responsibility:** *Know when power state changes; know nothing about graphics.*

---

### Component B — Visual Overlay Renderer

**Responsibility:** Create a transient, full-screen (or near full-screen), **click-through**, **non-focus-stealing** transparent window and play the rainbow shimmer animation for a configurable duration, then tear down.

**Interface (conceptual):**

```rust
trait OverlayRenderer {
    fn play_shimmer(&self, config: ShimmerConfig) -> Result<(), OverlayError>;
    fn is_playing(&self) -> bool;
    fn cancel(&self);
}

struct ShimmerConfig {
    duration_ms: u32,
    opacity: f32,
    speed: f32,
}
```

**Behavior:**
- Show overlay only during animation (no persistent full-screen window).
- Render via a GPU fragment shader (rainbow gradient + animated noise/mask for "shimmer").
- Must not capture keyboard focus or block mouse clicks (click-through).
- Must not appear in the taskbar/dock (where the platform supports hiding).

**Single Responsibility:** *Draw the effect; know nothing about how power state is detected.*

---

## 3. Concrete Libraries & Native APIs

### Linux (v1 target)

#### Power detection

| Approach | API / Library | Notes |
|---|---|---|
| **Primary** | **UPower** over **D-Bus** (`org.freedesktop.UPower`, `/org/freedesktop/UPower/devices/...`) via `zbus` | Standard on GNOME, KDE, and most desktop distros; event-driven |
| **Fallback** | **`/sys/class/power_supply/*/online`** via `inotify` or short-interval poll | For minimal/container environments without UPower |
| **Session context** | **`logind`** D-Bus (`org.freedesktop.login1`) | Optional; useful for lid-close/sleep coordination later |

Key UPower properties: `Online`, `Type` (`Line-Power` vs `Battery`), `State`.

#### Overlay rendering

| Concern | API / Library | Notes |
|---|---|---|
| Window | **`winit`** | Transparent, undecorated, always-on-top window spanning monitor(s) |
| GPU draw | **`wgpu`** | Fragment shader produces rainbow shimmer; minimal CPU work |
| **X11** click-through | **`X11` Shape extension** + `_NET_WM_STATE` hints via `winit` raw-window-handle / `x11rb` | Sets input shape to empty so clicks pass through |
| **X11** compositing | `_NET_WM_WINDOW_OPACITY`, ARGB visual | Requires compositor (standard on modern desktops) |
| **Wayland** placement | **`wlr-layer-shell`** via `smithay-client-toolkit` or `gtk-layer-shell` | **Important:** Wayland cannot freely overlay the desktop the X11 way; layer-shell is the portable Wayland path (Sway, Hyprland, some others). GNOME/KDE may need separate protocol adapters later. |

> **Linux v1 scope recommendation:** Support **X11 + UPower** first (broadest compatibility path), with Wayland layer-shell as a fast-follow adapter behind the same `OverlayRenderer` trait.

---

### Windows (future adapter)

| Concern | API / Library |
|---|---|
| Power events | `RegisterPowerSettingNotification` with `GUID_ACDC_POWER_SOURCE` via the `windows` crate; or WMI `Win32_Battery` change events |
| Overlay window | `winit` + `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_TOPMOST \| WS_EX_NOACTIVATE` via raw window handle |
| DWM transparency | `DwmExtendFrameIntoClientArea` for per-pixel alpha |
| Multi-monitor | `EnumDisplayMonitors` or `winit` monitor API |

---

### macOS (future adapter)

| Concern | API / Library |
|---|---|
| Power events | IOKit `IOPSNotificationCreateRunLoopSource` / `IOPSCopyPowerSourcesInfo` via `objc2` / `core-foundation` bindings |
| Overlay window | `winit` or `raw-window-handle` + `NSWindow` with `NSWindowStyleMaskBorderless`, `ignoresMouseEvents = true`, `NSWindowLevelScreenSaver` or higher |
| Transparency | `NSWindow` `isOpaque = false`, `backgroundColor = .clear` |

---

### Cross-cutting dependencies

| Purpose | Library |
|---|---|
| Trait-based DI / composition | Manual constructor injection in `app` crate (no heavy DI framework needed) |
| Logging | `tracing` + `tracing-subscriber` |
| Error types | `thiserror` in adapters; map to domain errors at boundaries |
| Testing | `tokio::test`, mock `PowerEventSource` streams, snapshot tests for shader uniforms |

---

## 4. Proposed Project Folder Structure

Workspace layout enforces **Clean Architecture**: domain logic in `core`, platform code in adapter crates, wiring in `app`.

```
power-shimmer/
├── Cargo.toml                      # Workspace manifest
├── TECH_STACK.md                   # This document
├── README.md
│
├── crates/
│   ├── core/                       # Domain + application (zero platform deps)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── domain/
│   │       │   ├── mod.rs
│   │       │   ├── events.rs           # PowerEvent, ShimmerConfig
│   │       │   └── errors.rs
│   │       ├── ports/
│   │       │   ├── mod.rs
│   │       │   ├── power_source.rs     # trait PowerEventSource
│   │       │   └── overlay.rs          # trait OverlayRenderer
│   │       └── services/
│   │           ├── mod.rs
│   │           └── shimmer_orchestrator.rs  # connects power → overlay
│   │
│   ├── platform-linux/             # Linux adapter implementations
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── power/
│   │       │   ├── mod.rs
│   │       │   ├── upower.rs           # UPower + zbus
│   │       │   └── sysfs_fallback.rs
│   │       └── overlay/
│   │           ├── mod.rs
│   │           ├── wgpu_shimmer.rs     # shader + render loop
│   │           ├── x11_click_through.rs
│   │           └── wayland_layer_shell.rs  # future / feature-gated
│   │
│   ├── platform-windows/           # Stub crate (future)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── power/mod.rs
│   │       └── overlay/mod.rs
│   │
│   ├── platform-macos/             # Stub crate (future)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── power/mod.rs
│   │       └── overlay/mod.rs
│   │
│   └── app/                        # Composition root + binaries
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── config.rs               # Load TOML settings
│           ├── tray.rs                 # System tray integration
│           └── wiring.rs               # Instantiate adapters → orchestrator
│
├── assets/
│   └── shaders/
│       └── shimmer.wgsl                # Rainbow shimmer fragment shader
│
├── config/
│   └── default.toml                    # Default user settings
│
└── tests/
    ├── core/
    │   └── orchestrator_test.rs        # Unit tests with mock ports
    └── integration/
        └── linux_power_smoke.rs        # Optional; requires hardware/UPower
```

### Dependency direction (must not be violated)

```
app  →  platform-*  →  (OS APIs)
  ↘       ↗
    core  (traits + orchestrator only)
```

- `core` imports **nothing** from `platform-*`, `winit`, `zbus`, or `wgpu`.
- `platform-linux` implements `core::ports` traits.
- `app` is the only crate that knows which platform adapter to construct at startup.

### Cargo feature flags (proposed)

```toml
# app/Cargo.toml (conceptual)
[features]
default = ["linux"]
linux   = ["platform-linux"]
windows = ["platform-windows"]
macos   = ["platform-macos"]
```

---

## Key Risks & Mitigations

| Risk | Mitigation |
|---|---|
| **Wayland overlay restrictions** | Abstract `OverlayRenderer`; ship X11 first; add layer-shell adapter per compositor family |
| **Click-through behavior varies by WM** | Encapsulate in platform overlay module; integration-test on target distros (GNOME, KDE, i3) |
| **Plug/unplug event noise** | Debounce in Linux power adapter (e.g., 300–500 ms) before emitting `AcConnected` |
| **Multi-monitor layouts** | v1: animate on primary monitor only; v2: iterate `winit` monitors |
| **Battery API differences on Win/Mac** | Shared `PowerEvent` enum; each adapter maps native signals independently |

---

## Suggested Implementation Order (post-approval)

1. **`core` ports + orchestrator** with unit tests using mock streams.
2. **`platform-linux` UPower adapter** — log events to console.
3. **`platform-linux` overlay** — static rainbow shader, then animated shimmer.
4. **`app` wiring** — connect orchestrator, tray icon, config file.
5. **X11 click-through + opacity polish**.
6. **Wayland layer-shell adapter** (feature-gated).
7. **Stub `platform-windows` / `platform-macos`** crates with trait impl skeletons.

---

## Resolved Decisions

1. **Animation scope:** Primary monitor only (v1).
2. **Trigger policy:** Battery → AC transition only.
3. **Wayland:** v1.1 (X11 + UPower for v1).
4. **User controls:** System tray menu + CLI flags (`--trigger`, `--dry-run`, etc.).

Interface contracts are defined in [`SPEC.md`](SPEC.md).
