# Power Shimmer — Interface Specification

> **Status:** Approved — defines contracts for implementation.  
> **Companion:** [`TECH_STACK.md`](TECH_STACK.md)

This document specifies the exact interfaces and behavioral protocols for the three core modules. All types and traits live in the `core` crate unless noted. Platform adapters (`platform-linux`, etc.) and the composition root (`app`) implement or wire these contracts but must not leak OS-specific types across boundaries.

---

## Approved v1 Product Decisions

| Decision | Value |
|---|---|
| Monitor scope | **Primary monitor only** |
| Shimmer trigger (automatic) | **Battery → AC transition only** |
| Wayland overlay | **v1.1** (v1 targets X11 + UPower) |
| User controls | **System tray menu + CLI flags** |

---

## Architectural Boundaries

```
┌──────────────────────────────────────────────────────────────────┐
│  app (composition root)                                          │
│  • Parses CLI, loads config, builds adapters                       │
│  • Owns tokio runtime and task spawning                          │
└────────────┬───────────────────────────────┬─────────────────────┘
             │ implements                    │ implements
             ▼                               ▼
┌────────────────────────┐       ┌───────────────────────────────┐
│  PowerEventListener    │       │  OverlayRenderer              │
│  (platform-* crate)    │       │  (platform-* crate)           │
└────────────┬───────────┘       └───────────────┬───────────────┘
             │ implements                        │ implements
             ▼                                   ▼
┌──────────────────────────────────────────────────────────────────┐
│  core::ports::PowerEventListener   core::ports::OverlayRenderer  │
│  core::services::ShimmerOrchestrator                             │
│  core::domain::*  (pure value types — no I/O)                    │
└──────────────────────────────────────────────────────────────────┘
```

### Decoupling rules (mandatory)

1. **`core` has zero dependencies** on `winit`, `wgpu`, `zbus`, `tokio` (beyond optional `std`/`futures`-style abstractions if needed), or any `platform-*` crate.
2. **Power Listener never calls Overlay Renderer** — all coupling flows through the Orchestrator.
3. **Overlay Renderer never reads power state** — it only receives explicit `ShimmerRequest` values.
4. **Orchestrator never imports platform crates** — it depends only on port traits defined in `core`.
5. **Adapters translate, not decide** — platform code normalizes OS signals into domain events; trigger policy (Battery → AC) lives exclusively in the Orchestrator.
6. **Manual triggers bypass power policy** — CLI `--trigger` and tray "Play now" invoke the Orchestrator directly; they do not synthesize fake power events.

---

## Module 1 — Domain Types (`core::domain`)

Shared vocabulary used by all three modules. Pure data; no methods that perform I/O.

### `PowerSource`

Represents the laptop's active power feed at a point in time.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    /// Running on battery.
    Battery,
    /// AC adapter or line power connected.
    Ac,
    /// State could not be determined (adapter startup only).
    Unknown,
}
```

### `PowerEvent`

Factual reports from the Power Listener. These are **not** commands and carry **no** shimmer policy.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerEvent {
    /// Emitted once when the listener starts, before any transition events.
    /// Lets the Orchestrator seed its state without triggering a shimmer.
    InitialState { source: PowerSource },

    /// Emitted when the active power source changes.
    Transition {
        from: PowerSource,
        to: PowerSource,
    },
}

impl PowerEvent {
  /// Returns true for Transition { from: Battery, to: Ac } only.
  pub fn is_battery_to_ac(&self) -> bool;
}
```

**Invariant:** `Transition.from` must differ from `Transition.to`. Adapters must not emit no-op transitions.

**Invariant:** Exactly one `InitialState` is emitted per subscription before any `Transition`.

### `MonitorTarget`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorTarget {
    /// v1 default — primary display only.
    Primary,
    // Future: All, ById(u32)
}
```

### `ShimmerConfig`

User-tunable visual parameters. Loaded from config file; passed through Orchestrator to Overlay.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ShimmerConfig {
    /// Total animation length in milliseconds. Default: `2000`.
    pub duration_ms: u32,
    /// Peak overlay opacity in `[0.0, 1.0]`. Default: `0.35`.
    pub opacity: f32,
    /// Shimmer scroll speed multiplier. Default: `1.0`.
    pub speed: f32,
    /// Which display to cover. v1: always `Primary`.
    pub monitor: MonitorTarget,
}
```

### `ShimmerRequest`

A single play invocation. Constructed by the Orchestrator (automatic or manual).

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ShimmerRequest {
    pub config: ShimmerConfig,
    /// Distinguishes automatic vs user-initiated plays (logging/metrics only).
    pub trigger: ShimmerTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimmerTrigger {
    PowerTransition,
    Manual,
    Cli,
}
```

### `OrchestratorConfig`

Runtime policy for the Orchestrator (distinct from visual `ShimmerConfig`).

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct OrchestratorConfig {
    pub shimmer: ShimmerConfig,
    /// When false, automatic Battery→AC triggers are suppressed.
    /// Manual/CLI triggers still work unless `dry_run` is true.
    pub auto_enabled: bool,
    /// When true, Orchestrator logs intent but does not call OverlayRenderer.
    pub dry_run: bool,
    /// If a shimmer is already playing when a new request arrives:
    /// `Skip` ignores the new request; `Restart` cancels and replays.
    pub overlap_policy: OverlapPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapPolicy {
    Skip,
    Restart,
}
```

### Error types

```rust
#[derive(Debug, thiserror::Error)]
pub enum PowerListenerError {
    #[error("failed to subscribe to power events: {0}")]
    SubscribeFailed(String),
    #[error("power event stream ended unexpectedly")]
    StreamEnded,
}

#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error("overlay already disposed")]
    Disposed,
    #[error("failed to create overlay window: {0}")]
    WindowCreationFailed(String),
    #[error("render error: {0}")]
    RenderFailed(String),
    #[error("play cancelled")]
    Cancelled,
}

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("power listener error: {0}")]
    Power(#[from] PowerListenerError),
    #[error("overlay error: {0}")]
    Overlay(#[from] OverlayError),
}
```

---

## Module 2 — Power Listener (`core::ports::power`)

### Responsibility

Observe OS power state and emit normalized `PowerEvent` values on a subscription channel. The listener is **read-only** with respect to system power — it never inhibits sleep, changes power profiles, or interacts with graphics.

### Port trait

```rust
/// Async power event source. Implemented by platform adapters only.
pub trait PowerEventListener: Send + Sync {
    /// Begin emitting power events.
    ///
    /// Returns a fallible stream of `PowerEvent`. The implementation must:
    ///   1. Query current power source.
    ///   2. Emit `InitialState { source }`.
    ///   3. Emit `Transition { from, to }` on each subsequent change.
    ///
    /// The stream runs until the returned handle is dropped or the OS
    /// subscription ends (reported as `PowerListenerError::StreamEnded`).
    fn subscribe(
        &self,
    ) -> Result<PowerEventStream, PowerListenerError>;
}

/// Opaque handle combining event receiver + cleanup.
/// Dropping cancels the OS subscription.
pub struct PowerEventStream {
    // implementation detail in adapter; spec-level behavior only:
    // - recv() -> Option<Result<PowerEvent, PowerListenerError>>
    // - Drop triggers unsubscribe
}
```

> **Note for implementers:** At the Rust level, `PowerEventStream` may wrap `futures::Stream` or a `tokio::sync::mpsc::Receiver`. The `core` crate defines the behavioral contract; the concrete type alias lives in `core` and is constructed only by adapters.

### Adapter obligations (Linux v1 — UPower)

| Step | Behavior |
|---|---|
| Startup | Read `Online` / device `Type` from UPower D-Bus; map to `PowerSource` |
| First emit | `InitialState { source }` |
| On change | After debounce (see below), emit `Transition { from, to }` |
| Mapping | `Online == true` → `Ac`; `Online == false` → `Battery` |
| Debounce | **400 ms** — coalesce rapid plug/unplug flicker before emitting a `Transition` |
| Unknown | If UPower unavailable at startup, emit `InitialState { Unknown }`; retry subscribe in adapter background (adapter detail; must not block Orchestrator) |

### Adapter obligations (Linux fallback — sysfs)

Same event protocol as UPower. Poll or `inotify` on `/sys/class/power_supply/*/online`. Feature-gated or auto-selected when D-Bus session unavailable.

### What the Power Listener must NOT do

- Decide whether a shimmer should play.
- Filter out `Ac → Ac` (impossible if transitions are edge-triggered).
- Call or reference `OverlayRenderer`.
- Expose D-Bus paths, sysfs paths, or `zbus` types in public signatures.

### Test double contract

```rust
#[cfg(test)]
pub struct MockPowerEventListener {
    events: Vec<PowerEvent>,
}

// Injects a predetermined Vec<PowerEvent> for Orchestrator unit tests.
```

---

## Module 3 — Overlay Renderer (`core::ports::overlay`)

### Responsibility

Given a `ShimmerRequest`, create a transient overlay on the target monitor, animate the rainbow shimmer, then destroy all resources. The renderer is stateless with respect to power — it only responds to explicit play/cancel invocations.

### Port trait

```rust
/// Visual overlay port. Implemented by platform adapters only.
pub trait OverlayRenderer: Send + Sync {
    /// Play one shimmer animation.
    ///
    /// Contract:
    ///   - Creates window + GPU resources lazily if not present.
    ///   - Covers `request.config.monitor` (v1: primary display bounds).
    ///   - Window is click-through, does not take focus, hidden from taskbar.
    ///   - Animates for `request.config.duration_ms`, then hides window.
    ///   - Returns `Ok(())` on normal completion.
    ///   - Returns `Err(OverlayError::Cancelled)` if `cancel()` was called mid-play.
    ///
    /// Must be safe to call from a dedicated async task (Orchestrator-owned).
    async fn play(&self, request: ShimmerRequest) -> Result<(), OverlayError>;

    /// Returns true while `play()` is in progress (including fade-out teardown).
    fn is_playing(&self) -> bool;

    /// Abort the current animation immediately and release the overlay window.
    /// No-op if idle. After cancel, `is_playing()` becomes false.
    fn cancel(&self);
}
```

### Visual contract (v1)

| Property | Requirement |
|---|---|
| Window placement | Full bounds of **primary monitor** |
| Input | Mouse and keyboard pass through to apps below |
| Focus | Never becomes key window |
| Taskbar | Not listed (platform best-effort) |
| Background | Fully transparent outside shimmer band |
| Animation | GPU fragment shader; rainbow spectrum + moving highlight ("shimmer") |
| Duration | Honours `duration_ms` ± one frame (~16 ms) |
| Teardown | GPU + window resources released within **500 ms** of completion/cancel |

### Completion protocol

```
Caller (Orchestrator)                OverlayRenderer
        |                                    |
        |  play(ShimmerRequest)              |
        |----------------------------------->|
        |                                    | create/show window
        |                                    | render frames
        |                                    | hide/destroy window
        |            Ok(()) or Err(...)      |
        |<-----------------------------------|
        |                                    |
        |  cancel()  (optional, mid-flight)  |
        |----------------------------------->|
        |                                    | abort, teardown
        |            (play returns Cancelled)|
        |<-----------------------------------|
```

### What the Overlay Renderer must NOT do

- Subscribe to power events.
- Read config files or parse CLI flags (receives fully-built `ShimmerRequest`).
- Encode Battery → AC policy.
- Expose `winit::Window`, `wgpu::Device`, or platform handles in public API.

### Test double contract

```rust
#[cfg(test)]
pub struct MockOverlayRenderer {
    pub play_calls: Arc<Mutex<Vec<ShimmerRequest>>>,
    pub delay: Duration,
}

// Records requests; simulates async completion after `delay`.
// Supports cancel() returning Cancelled from in-flight play().
```

---

## Module 4 — Shimmer Orchestrator (`core::services`)

### Responsibility

Single application-service that:

1. Consumes the Power Listener event stream.
2. Applies **Battery → AC** automatic trigger policy.
3. Invokes Overlay Renderer for qualifying events and manual commands.
4. Enforces overlap policy and dry-run mode.

The Orchestrator is the **only** module that coordinates both ports.

### Public interface

```rust
pub struct ShimmerOrchestrator<P, O>
where
    P: PowerEventListener,
    O: OverlayRenderer,
{
    // fields private
}

impl<P, O> ShimmerOrchestrator<P, O>
where
    P: PowerEventListener,
    O: OverlayRenderer,
{
    pub fn new(power: P, overlay: O, config: OrchestratorConfig) -> Self;

    /// Process power events until `shutdown()` is called or the stream ends.
    /// Runs automatic trigger logic. Intended to be spawned as a tokio task.
    pub async fn run(&mut self) -> Result<(), OrchestratorError>;

    /// Manually trigger a shimmer (tray "Play now", hotkey future).
    pub async fn trigger_manual(&mut self) -> Result<(), OrchestratorError>;

    /// Update runtime config (tray toggle, config reload).
    pub fn update_config(&mut self, config: OrchestratorConfig);

    /// Enable/disable automatic Battery→AC triggers without stopping the run loop.
    pub fn set_auto_enabled(&mut self, enabled: bool);

    /// Cancel in-flight shimmer and signal `run()` to exit cleanly.
    pub async fn shutdown(&mut self);
}
```

### Internal state (Orchestrator-owned)

```rust
struct OrchestratorState {
    /// Last known power source; seeded from InitialState.
    current_power: PowerSource,
    /// Guards against duplicate triggers before InitialState arrives.
    power_initialized: bool,
}
```

### Automatic trigger state machine

```
                    ┌─────────────────┐
                    │  await event    │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
       InitialState    Transition      (stream end)
              │              │              │
              │              │              └──▶ Err(StreamEnded)
              ▼              ▼
    seed current_power   update current_power
    (no shimmer)              │
                              ▼
                    is Battery → Ac ?
                     /              \
                   no                yes
                   │                  │
              (no shimmer)     auto_enabled && !dry_run ?
                                /              \
                              no                yes
                              │                  │
                         (log skip)         invoke play()
```

**Automatic play predicate (exact):**

```rust
fn should_auto_play(event: &PowerEvent, config: &OrchestratorConfig) -> bool {
    event.is_battery_to_ac()
        && config.auto_enabled
        && !config.dry_run
}
```

**Examples:**

| Sequence | Shimmer? |
|---|---|
| Boot on AC → `InitialState(Ac)` | No |
| Boot on battery → `InitialState(Battery)` | No |
| `Transition(Battery, Ac)` | **Yes** (if auto enabled) |
| `Transition(Ac, Battery)` | No |
| `Transition(Battery, Battery)` | Invalid — adapter bug |
| `Transition(Ac, Ac)` | Invalid — adapter bug |
| Second `Transition(Battery, Ac)` without unplug | Only if user unplugged (→ Battery) in between |

### Manual trigger protocol

```rust
// trigger_manual / CLI --trigger
async fn trigger_manual(&mut self) -> Result<(), OrchestratorError> {
    if self.config.dry_run {
        log::info!("dry-run: would play shimmer (manual)");
        return Ok(());
    }
    self.play_shimmer(ShimmerTrigger::Manual).await
}
```

Manual triggers **ignore** `auto_enabled` but **respect** `dry_run` and `overlap_policy`.

### Overlap handling

Before calling `overlay.play()`:

```rust
if overlay.is_playing() {
    match config.overlap_policy {
        OverlapPolicy::Skip => return Ok(()),
        OverlapPolicy::Restart => overlay.cancel(),
    }
}
overlay.play(request).await?;
```

Default for v1: **`OverlapPolicy::Skip`**.

### Play helper (internal)

```rust
async fn play_shimmer(
    &mut self,
    trigger: ShimmerTrigger,
) -> Result<(), OrchestratorError> {
    let request = ShimmerRequest {
        config: self.config.shimmer.clone(),
        trigger,
    };
  // overlap check, then overlay.play(request).await
}
```

### Shutdown protocol

1. Set internal shutdown flag.
2. Call `overlay.cancel()` if playing.
3. Drop power event stream handle (unsubscribe).
4. `run()` returns `Ok(())`.

---

## App-Layer Integration (reference — not part of `core`)

The `app` crate wires components together. Documented here so CLI/tray behavior is unambiguous.

### CLI flags (v1)

| Flag | Effect |
|---|---|
| *(none)* | Run daemon: tray + Orchestrator `run()` loop |
| `--trigger` | Play shimmer once via `trigger_manual()`, then exit (no tray, no power loop) |
| `--dry-run` | Orchestrator sets `dry_run = true`; log triggers, no overlay |
| `--no-tray` | Skip tray icon; run headless daemon with power loop |
| `--duration-ms <n>` | Override `ShimmerConfig.duration_ms` |
| `--opacity <f>` | Override `ShimmerConfig.opacity` |

`--trigger` and daemon mode are mutually exclusive entry paths in `main()`.

### Tray menu actions (v1)

| Item | Orchestrator call |
|---|---|
| Play now | `trigger_manual().await` |
| Enable / Disable auto shimmer | `set_auto_enabled(true/false)` |
| Quit | `100` → `shutdown().await` |

---

## Crate Dependency Matrix

| Crate | May depend on |
|---|---|
| `core` | `thiserror` only (no async runtime in public API surface if avoidable) |
| `platform-linux` | `core`, `tokio`, `zbus`, `winit`, `wgpu`, … |
| `app` | `core`, `platform-*`, `tokio`, `tray-icon`, `clap`, `serde` |

---

## File Mapping (implementation reference)

| Spec module | `core` path |
|---|---|
| Domain types | `crates/core/src/domain/{events,config,errors}.rs` |
| Power Listener port | `crates/core/src/ports/power.rs` |
| Overlay Renderer port | `crates/core/src/ports/overlay.rs` |
| Shimmer Orchestrator | `crates/core/src/services/shimmer_orchestrator.rs` |

| Platform adapter | Path |
|---|---|
| Linux UPower listener | `crates/platform-linux/src/power/upower.rs` |
| Linux X11 overlay | `crates/platform-linux/src/overlay/wgpu_shimmer.rs` |

---

## Unit Test Requirements (before platform impl)

The following tests must pass against mock ports **before** real adapters ship:

### Power Listener (mock)

- Emits `InitialState` before any `Transition`.
- Adapter test (Linux): debounce coalesces two events within 400 ms into one `Transition`.

### Orchestrator

| Test case | Expected |
|---|---|
| `InitialState(Ac)` | 0 overlay plays |
| `InitialState(Battery)` then `Transition(Battery, Ac)` | 1 play, trigger = `PowerTransition` |
| `Transition(Ac, Battery)` | 0 plays |
| `auto_enabled = false` + Battery→Ac | 0 plays |
| `dry_run = true` + Battery→Ac | 0 plays, log only |
| `trigger_manual()` on battery | 1 play, trigger = `Manual` |
| Overlap `Skip` + second trigger while playing | 1 play total |
| Overlap `Restart` + second trigger while playing | 2 plays sequential |

### Overlay Renderer (mock)

- `play()` sets `is_playing` true until completion.
- `cancel()` during play returns `OverlayError::Cancelled`.
- Completed play sets `is_playing` false.

---

## Version Roadmap Hooks

| Feature | Spec impact |
|---|---|
| **v1.1 Wayland** | New adapter impl of `OverlayRenderer`; no trait change |
| **Multi-monitor** | Add `MonitorTarget::All` / `ById`; Orchestrator passes through config |
| **Windows / macOS** | New `PowerEventListener` + `OverlayRenderer` impls; no `core` change |

---

*This specification is the authoritative contract for implementation. Changes require explicit revision to this document.*
