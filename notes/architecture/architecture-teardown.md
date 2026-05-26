# Power Shimmer — Architectural Teardown (core / app / platform-linux)

This note is a **plain-English, boundary-focused** explanation of how the repository is structured today, what the **core trait contracts (ports)** mean, how data moves from a **physical Linux power transition** all the way into the **GPU shader**, and how **sync threads** communicate with **async Tokio tasks**.

Scope: `crates/core`, `crates/app`, `crates/platform-linux` only.

---

## Mental model: hexagonal-ish, but very small

- **`core`** defines:
  - *Domain types* (`PowerEvent`, `ShimmerRequest`, configs, errors).
  - *Ports* (traits) that describe what “the outside world” must provide.
  - *The orchestrator* that is the only place where power events and overlay playback meet.

- **`platform-linux`** provides:
  - Adapters that implement the **core ports** for Linux.
  - Concrete rendering (winit + wgpu + WGSL shader).
  - Concrete power sources (UPower over D-Bus, sysfs fallback).

- **`app`** is the composition root:
  - Picks the Linux adapters, builds the orchestrator, spawns `run()` on Tokio, optionally runs a tray loop.

The “boundary” is simple: **core can talk to ports; platform implements ports**. `app` wires them together.

---

## The Core Trait Contracts (Ports)

In this repo, “ports” are small, very explicit trait contracts that let the `core` orchestrator stay OS-agnostic.

### Port: `OverlayRenderer`

**What it is**: a “visual overlay player” that can play exactly one shimmer animation at a time.

**Where it lives**: `crates/core/src/ports/overlay.rs`

**Who implements it (Linux)**: `WgpuShimmerRenderer` in `crates/platform-linux/src/overlay/wgpu_shimmer.rs`

**Methods and contract (practical meaning)**:

- `async fn play(&self, request: ShimmerRequest) -> Result<(), OverlayError>`
  - **Input**: `ShimmerRequest` (contains `ShimmerConfig` + `ShimmerTrigger`).
  - **Output**:
    - `Ok(())` means the animation ran to completion and the window was hidden/teardown finished (best effort).
    - `Err(OverlayError::Cancelled)` means someone called `cancel()` during the play.
    - Other `OverlayError`s indicate failures to create a window, get a GPU adapter/device, present frames, etc.
  - **Important behavioral guarantees (from the contract + implementation intent)**:
    - Creates window/GPU resources lazily.
    - Covers `request.config.monitor` (v1: “Primary”; Linux currently chooses `primary_monitor()` or first available).
    - Click-through, non-focus, hidden-from-taskbar (best effort; Linux X11 hints).
    - Must be safe to call from a dedicated async task owned by the orchestrator.

- `fn is_playing(&self) -> bool`
  - **Output**: `true` while a play is active, including teardown.
  - **Why it exists**: it lets the orchestrator apply **overlap policy** (`Skip` vs `Restart`) without needing renderer internals.

- `fn cancel(&self)`
  - **Effect**: aborts the current animation immediately (no-op if idle).
  - **Postcondition**: eventually `is_playing()` becomes `false` (immediately or after teardown).

**Key non-goals of this port**:
- It does **not** make policy decisions (when to play, overlap behavior, power logic).
- It does **not** expose GPU types (`wgpu::Device`, etc.) or windowing types to `core`.

---

### Port: `PowerEventListener`

**What it is**: a subscription-based source of normalized “power source changed” facts.

**Where it lives**: `crates/core/src/ports/power.rs`

**Who implements it (Linux)**: `LinuxPowerListener<B>` in `crates/platform-linux/src/power/listener.rs`

**Core method and contract**:

- `fn subscribe(&self) -> Result<PowerEventStream, PowerListenerError>`
  - **Output (success)**: a `PowerEventStream` that delivers `Result<PowerEvent, PowerListenerError>`.
  - **Required behavior**:
    1. Query current power source.
    2. Emit **exactly one** `PowerEvent::InitialState { source }`.
    3. Emit `PowerEvent::Transition { from, to }` for subsequent changes.
  - **Error**:
    - `SubscribeFailed` means the adapter cannot begin an OS subscription.
    - `StreamEnded` means the stream terminated unexpectedly after having started.

**What is `PowerEventStream`, really?**

Despite the name, it is not an async `Stream`. It is:
- a **std** `mpsc::Receiver<Result<PowerEvent, PowerListenerError>>`
- plus an adapter-owned worker `JoinHandle<()>` stored so it lives as long as the subscription

The orchestrator polls it with:
- `recv()` (blocking) or
- `recv_timeout(timeout)` which returns `Message(...) | Timeout | Disconnected`

**Why this design matters**:
- The port deliberately allows platform adapters to use **blocking OS APIs**, dedicated threads, and plain channels.
- `core` stays free of OS subscription concerns; it just “ticks” and processes received facts.

---

## Data Flow Topology (Linux power transition → orchestrator → GPU shader)

We’ll trace one real-world event: “laptop unplugged → plugged back into AC”.

### High-level topology (who talks to whom)

```
┌────────────────────┐
│  Linux (kernel/OS) │
└─────────┬──────────┘
          │ (UPower D-Bus signals OR sysfs changes)
          v
┌────────────────────────────┐
│ platform-linux power backend│  (UpowerBackend or SysfsFallbackBackend)
└─────────┬──────────────────┘
          │ (change hint)
          v
┌────────────────────────────┐
│ LinuxPowerListener worker   │  (std::thread + std::mpsc)
└─────────┬──────────────────┘
          │ PowerEvent::Transition
          v
┌────────────────────────────┐
│ core ShimmerOrchestrator    │  (Tokio task calling run().await)
└─────────┬──────────────────┘
          │ ShimmerRequest
          v
┌────────────────────────────┐
│ platform-linux OverlayRenderer│ (WgpuShimmerRenderer)
└─────────┬──────────────────┘
          │ OverlayUserEvent::Play (winit proxy)
          v
┌────────────────────────────┐
│ Overlay thread: winit loop  │  (Window + wgpu device/queue/pipeline)
└─────────┬──────────────────┘
          │ uniform updates + draw calls
          v
┌────────────────────────────┐
│ GPU fragment shader (WGSL)  │  (assets/shaders/shimmer.wgsl)
└────────────────────────────┘
```

Now the concrete execution steps.

---

### Step-by-step: from physical change to `PowerEvent`

#### 1) OS changes state

When AC is plugged/unplugged, the OS will:
- update kernel power_supply state (sysfs), and/or
- emit UPower property changes via the system D-Bus.

#### 2) `LinuxPowerBackend::select()` picks an implementation

In `app` wiring (`crates/app/src/wiring.rs`), construction is:
- `let backend = LinuxPowerBackend::select();`
- `let power = LinuxPowerListener::new(backend);`

`LinuxPowerBackend` is an enum:
- `Upower(UpowerBackend)` when reachable and providing a reading
- otherwise `Sysfs(SysfsFallbackBackend)`

#### 3) Backend produces “change hints” and a settled `online` value

The backend trait used by `LinuxPowerListener` is:

- `wait_online_change()` — **blocks** until the backend believes online state *may* have changed.
- `read_online()` — reads the **current settled** `Option<bool>` (`None` means “unknown”).
- `try_wait_online_change(timeout)` — optionally waits for more hints (used for debouncing).

For UPower specifically (`UpowerBackend`):
- A dedicated **monitor thread** runs a small **current-thread Tokio runtime** and holds a D-Bus connection.
- Property-change signals cause the backend to:
  - read the latest “AC online” state,
  - store it in `online: Mutex<Option<bool>>`,
  - send `()` on a std `mpsc` channel as a “change hint”.

The important point: the backend does **not** produce domain events; it produces:
- **(a)** a synchronized `online` reading and
- **(b)** hints that “you should re-check now”.

#### 4) `LinuxPowerListener` normalizes into domain events

`LinuxPowerListener::subscribe()` spawns a **worker thread** that:

1. Calls `backend.initial_source()` and immediately sends:
   - `PowerEvent::InitialState { source }`
2. Enters a loop:
   - `wait_online_change()` blocks until a hint arrives
   - performs a **debounce/coalesce** window (default 400ms) by calling `try_wait_online_change(remaining)`
   - calls `read_online()` to get a stable `Option<bool>`
   - maps to domain `PowerSource` (`Ac`/`Battery`/`Unknown`)
   - if it differs from the last sent value, sends:
     - `PowerEvent::Transition { from, to }`

Those events are sent over a **std** `mpsc::Sender` into the `PowerEventStream` receiver.

---

### Step-by-step: `PowerEvent` to `ShimmerRequest` to overlay play

#### 5) Orchestrator `run()` polls the stream on a Tokio task

In `app`, the orchestrator is spawned:
- `tokio::spawn(async move { orchestrator.run().await ... })`

Inside `ShimmerOrchestrator::run()`:

- It calls `power.subscribe()` once.
- It stores the stream internally.
- It loops until shutdown:
  - calls `stream.recv_timeout(50ms)`
  - if it receives an event:
    - `InitialState`: update internal state only (tracks current power source).
    - `Transition`: update internal state; then checks trigger policy; if allowed, calls `play_shimmer(...)`.

**Ownership note**: the orchestrator owns the *ports* (the listener and renderer), and it owns the `ShimmerRequest` values it constructs, but it does not own any OS/GPU resources directly.

#### 6) Policy: “should we play?”

Policy is centralized in `core`:
- `should_auto_play(&event, &config)` decides whether a `Transition` should trigger a shimmer.

This keeps “what happened” (PowerEvent) separate from “what to do about it” (policy).

#### 7) Overlap behavior is enforced in `core`

Before calling the overlay, `core` checks whether something is already playing:

- If `overlay.is_playing()` is `false` → proceed.
- If `overlay.is_playing()` is `true` → consult `OverlapPolicy` from `OrchestratorConfig`:
  - `Skip` (v1 default): do nothing and return `Ok(())`.
  - `Restart`: call `overlay.cancel()` and then proceed to replay.

This is a key boundary point:
- **The renderer does not decide overlap.**
- **The orchestrator does not do rendering.**

#### 8) Orchestrator constructs the one “command” it sends across the boundary

When it decides to play, `core` builds a single value:

- `ShimmerRequest { config: ShimmerConfig, trigger: ShimmerTrigger }`

Then it calls:
- `overlay.play(request).await`

At this moment, control crosses from:
- “policy + orchestration world” (core)
to
- “windowing + GPU world” (platform-linux)

---

### Step-by-step: from `OverlayRenderer::play()` to GPU shader execution

#### 9) `WgpuShimmerRenderer::play()` is async, but it delegates work to a dedicated overlay thread

Linux’s `WgpuShimmerRenderer` is constructed once in `app` (`WgpuShimmerRenderer::new()`), and that constructor:
- creates a shared `SessionController` (atomics + small mutex)
- spawns a dedicated OS thread named `power-shimmer-overlay`
- creates a winit event loop on that thread
- captures an `EventLoopProxy<OverlayUserEvent>` back to the main world

So by the time `play()` is called, the overlay thread already exists and is running.

When `play(request)` runs:

1. It checks environment constraints:
   - v1 overlay requires X11 (`DISPLAY` must exist; Wayland-only without XWayland is rejected).
2. It waits briefly for teardown grace if the previous session is still marked “playing”.
3. It allocates a new `SessionId` via the `SessionController`.
4. It creates a Tokio `oneshot` channel:
   - **sender** lives on the overlay thread
   - **receiver** is awaited by `play()`
5. It sends a `OverlayUserEvent::Play { request, session_id, done }` into the overlay thread via `EventLoopProxy`.
6. It awaits the oneshot result, with a 15s timeout:
   - success → returns whatever the overlay thread reported (`Ok` or `Err(Cancelled)` etc.)
   - timeout → calls `cancel()` and returns an overlay error

**Ownership note**:
- The `ShimmerRequest` is *moved* into the user event and becomes owned by the overlay thread.
- The orchestrator keeps no pointer/borrow into it.

#### 10) Overlay thread receives `OverlayUserEvent::Play` and builds a `GpuSession`

On the overlay thread, `OverlayApp::user_event` handles `Play` by calling `start_session(...)`.

`start_session` does “everything OS/GPU”:

- **Monitor selection**: `primary_monitor()` or first available
- **Window creation**:
  - transparent, borderless, always-on-top, borderless fullscreen on target monitor
  - set invisible initially; then shown once GPU is ready
  - apply X11 “click-through overlay hints” best-effort
- **wgpu initialization**:
  - create `wgpu::Instance`
  - create a `Surface` from the window
  - request an adapter (`LowPower`, surface-compatible)
  - request device + queue
  - choose an SRGB format if possible; otherwise a preferred fallback
  - configure the surface (premultiplied alpha if possible)
- **Pipeline creation**:
  - compile embedded WGSL (`assets/shaders/shimmer.wgsl` is `include_str!`’d)
  - create a render pipeline and a uniform buffer

Finally it stores everything in:
- `self.gpu = Some(GpuSession { window, surface, device, queue, pipeline, request, session_id, started, done })`

At this point the overlay is “armed” and the render loop drives it.

#### 11) The render loop is winit’s redraw cycle

- Each time winit delivers `WindowEvent::RedrawRequested`, the app calls `render_frame()`.
- `render_frame()` is the “heartbeat” of the animation.

`render_frame()` does these checks in order:

1. **Cancellation check**:
   - reads `SessionController` cancellation flag for the session id
   - if cancelled → finish with `OverlayError::Cancelled`
2. **Duration check**:
   - compares `elapsed` with `request.config.duration_ms`
   - if elapsed is at/near end → finish with `Ok(())`
3. **Resize check**:
   - if window size changed → reconfigure surface
4. **Uniform update**:
   - packs `ShimmerParams { elapsed_s, duration_s, opacity, speed }` from config + elapsed
   - writes it into the GPU uniform buffer with `queue.write_buffer(...)`
5. **Draw + present**:
   - acquire swapchain texture (`surface.get_current_texture()`)
   - encode a render pass:
     - clear to transparent
     - draw a fullscreen triangle (`draw(0..3, 0..1)`)
   - submit to queue and present
6. **Schedule next frame**:
   - `window.request_redraw()`

#### 12) The GPU shader is parameterized only by a tiny uniform block

The shader inputs (CPU side) are defined as `ShimmerParams`:

- `elapsed_s`: seconds since start
- `duration_s`: total duration
- `opacity`: peak opacity
- `speed`: speed multiplier

These are the *only* per-frame dynamic values the CPU sends.

The fragment shader in `assets/shaders/shimmer.wgsl` is what ultimately decides:
- how the shimmer gradient moves over time
- how opacity ramps over time
- what pixels are transparent vs lit

---

## “Who owns the memory? Who triggers whom?”

This is the part that makes audits safe: you want to know **what can outlive what**, and **which thread is allowed to touch which resource**.

### Ownership map (power path)

```
UpowerBackend
  owns: monitor thread + SharedState
    SharedState owns:
      online: Mutex<Option<bool>>
      change channel (std mpsc)

LinuxPowerListener
  owns: Arc<backend>
  on subscribe(): spawns worker thread
    worker thread owns: std mpsc Sender<Result<PowerEvent, ...>>

PowerEventStream
  owns: std mpsc Receiver<Result<PowerEvent, ...>>
  owns: JoinHandle<()> (keeps worker alive)

ShimmerOrchestrator
  owns: ports + Mutex<Option<PowerEventStream>>
  polls receiver via recv_timeout(...)
```

**Trigger direction**:
- OS → backend (signals / file changes)
- backend → listener worker (hint)
- listener worker → stream receiver (PowerEvent)
- orchestrator → overlay port (`play` / `cancel`)

### Ownership map (overlay path)

```
WgpuShimmerRenderer
  owns: Arc<SessionController>
  owns: EventLoopProxy<OverlayUserEvent>
  owns: overlay OS thread (indirectly; thread is detached but lives for process)

Overlay thread (winit loop)
  owns: OverlayApp
    owns: Option<GpuSession>
      owns: Window, Surface, Device, Queue, Pipeline, ShimmerRequest, oneshot Sender

Tokio task calling play()
  owns: oneshot Receiver
  awaits overlay result
```

**Important rule**: **all winit + wgpu objects live and are mutated only on the overlay thread**.
The rest of the program communicates with that thread by sending **small messages** (`OverlayUserEvent`) and reading **atomics** (`SessionController` flags).

---

## The Concurrency Model (sync loops ↔ async Tokio tasks)

There are *multiple concurrency “worlds”* in this codebase:

- **Tokio async world**: orchestrator `run()`, manual trigger tasks, UPower’s internal async D-Bus session (inside a dedicated thread runtime).
- **Blocking thread world**:
  - power listener worker thread (std)
  - UPower monitor thread (std) hosting a current-thread Tokio runtime
  - overlay thread running winit (std)
  - tray GTK thread running a polling loop (std)

The design principle used here is:
> **Async code does not directly own or mutate UI/windowing/GPU state.**  
> It sends commands into dedicated threads and awaits completion via channels.

### Bridge 1: Blocking power listener → async orchestrator

Mechanism:
- `std::sync::mpsc` channel inside `PowerEventStream`
- orchestrator polls with `recv_timeout(50ms)`

Properties:
- **Backpressure**: std mpsc is unbounded; if events were produced faster than consumed, memory could grow. In practice power events are extremely low frequency.
- **Tokio interaction**: `recv_timeout` blocks the calling thread for up to 50ms. Because the orchestrator is spawned as its own task, the “damage” is bounded, but it still occupies a Tokio worker thread during that wait interval.

### Bridge 2: Async orchestrator → overlay thread (winit)

Mechanisms:
- `EventLoopProxy<OverlayUserEvent>` for commands
- `tokio::sync::oneshot` for completion
- `SessionController` atomics for `is_playing` and cancel routing

Why this is safe:
- winit requires a consistent thread model; the overlay thread is the only place that creates/owns the window and runs the event loop.
- async callers never touch those objects; they only send user events (which are owned by the overlay thread once delivered).

### Bridge 3: GTK tray thread → Tokio async tasks

Mechanism:
- the tray thread captures `tokio::runtime::Handle::current()` from the main async context
- when the user clicks “Play now”, it does `runtime.spawn(async move { orch.trigger_manual().await ... })`

Meaning:
- The tray loop is blocking and GTK-bound, but it can still schedule async work onto the existing runtime without creating a second global runtime.
- Shutdown signaling is synchronous: tray thread calls `orchestrator.shutdown()` directly (atomic flag + cancel).

### Cancellation semantics (how “stop” propagates)

There are two distinct “stop” paths:

- **Stop the orchestrator loop**:
  - `ShimmerOrchestrator::shutdown()` sets `shutdown_requested = true`
  - run loop notices and exits
  - it also cancels any in-flight shimmer (`overlay.cancel()`)
  - it drops the stored `PowerEventStream` (ending subscription ownership on the core side)

- **Stop a running overlay session**:
  - `overlay.cancel()` sets a cancel flag in `SessionController` and sends `OverlayUserEvent::Cancel { session_id }`
  - the overlay thread checks cancellation on every frame and/or handles the cancel event
  - it finishes the session, drops GPU resources, and resolves the oneshot with `OverlayError::Cancelled`

---

## Summary: boundaries you can audit

If you want to audit PRs by boundary invariants, these are the “must hold” statements:

- `core`:
  - knows **domain facts** and **policy**, and calls ports
  - does **not** import Linux windowing, D-Bus, wgpu, gtk, winit types

- `platform-linux`:
  - implements ports
  - owns OS resources and threads (winit loop, D-Bus connection)
  - does **not** contain shimmer policy (“when to play”) beyond debouncing the raw signal into stable transitions

- `app`:
  - wires production implementations together
  - owns process lifetime decisions (daemon vs trigger, tray vs headless)

