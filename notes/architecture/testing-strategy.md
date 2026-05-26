# Power Shimmer — Testing Strategy & Architecture

This note explains **how we test a decoupled Rust application** without mixing business logic with OS, GPU, or windowing concerns. It is written for PR review: you should be able to read a proposed test and immediately know whether it belongs in `core`, `platform-linux`, or a manual smoke tier.

**Plan Mode scope:** conceptual only — no production code changes suggested here.

**Companion doc:** runtime architecture and data flow live in [`architecture-teardown.md`](architecture-teardown.md).

---

## The testing pyramid (three tiers)

We deliberately split tests by **how much of the real world** they touch:

```
                    ┌─────────────────────────┐
                    │  Manual / smoke (ignored)│  DISPLAY, GPU, UPower hardware
                    │  platform-linux/tests/   │
                    └───────────┬─────────────┘
                                │
              ┌─────────────────┴─────────────────┐
              │  Adapter boundary tests          │  Fake OS backends, temp sysfs,
              │  platform-linux (#[cfg(test)])   │  mock D-Bus buses — no shimmer UI
              └─────────────────┬─────────────────┘
                                │
        ┌───────────────────────┴───────────────────────┐
        │  Pure core tests (in-memory)                   │  Mock ports only
        │  core: policy, mocks, orchestrator_test.rs    │  No Linux, no wgpu, no winit
        └───────────────────────────────────────────────┘
```

**Default CI** (`make test` → `cargo test --workspace --all-targets`) runs everything **except** tests marked `#[ignore]`. That keeps CI fast and deterministic while still compiling smoke tests for developers who opt in.

---

## 1. The Core Unit Test Matrix (`core` crate)

### What we are proving

The `core` crate owns **facts** (`PowerEvent`), **policy** (`should_auto_play`, overlap rules), and **orchestration** (`ShimmerOrchestrator`). Its tests answer:

> “Given a sequence of power facts and a config, does the orchestrator call the overlay the right number of times, with the right trigger, and handle overlap/shutdown correctly?”

They do **not** answer whether UPower, sysfs, X11, or wgpu work on your machine. That is intentional.

### Where the tests live

| Layer | Location | What it tests |
|-------|----------|----------------|
| Pure policy | `crates/core/src/services/policy.rs` (`#[cfg(test)]`) | `should_auto_play` truth table in isolation |
| Mock port contracts | `crates/core/src/testing/mock_*.rs` | Test doubles honor `PowerEventListener` / `OverlayRenderer` semantics |
| Orchestrator behavior table | `crates/core/tests/orchestrator_test.rs` | End-to-end orchestrator loop against mocks (SPEC table) |
| Config defaults | `crates/core/src/domain/config.rs` | Default `OrchestratorConfig` matches spec |

The orchestrator integration file is explicitly tied to **SPEC.md “Unit Test Requirements”** — it is the executable version of the behavior table.

### The SPEC behavior table (what `orchestrator_test.rs` enforces)

| Scenario | Expected |
|----------|----------|
| `InitialState(Ac)` only | 0 overlay plays |
| `InitialState(Battery)` then `Transition(Battery → Ac)` | 1 play, trigger = `PowerTransition` |
| `Transition(Ac → Battery)` | 0 plays |
| `auto_enabled = false` + Battery→Ac | 0 plays |
| `dry_run = true` + Battery→Ac | 0 plays |
| `trigger_manual()` (no power events needed) | 1 play, trigger = `Manual` |
| Overlap `Skip` + second trigger while playing | 1 play total |
| Overlap `Restart` + second trigger while playing | 2 plays (sequential) |
| `shutdown()` while run loop active | `run()` returns `Ok(())` cleanly |

When a PR touches orchestrator or policy, **this table is the regression contract**. If behavior changes, the test and SPEC row should change together.

### How test doubles isolate `core` from the real world

#### `MockPowerEventListener`

- **Implements:** `PowerEventListener` (same port trait production uses).
- **Mechanism:** On `subscribe()`, spawns a std thread that pushes a **predetermined `Vec<PowerEvent>`** into a real `PowerEventStream` (std `mpsc`, same type production uses).
- **Variants:**
  - `new(events)` — emit events, then **close** the stream (orchestrator eventually sees `StreamEnded`).
  - `keep_alive_after_events(events)` — stream stays open (for shutdown tests).

**Why this isolates OS:** No D-Bus, no sysfs, no kernel. You are only testing “if the port delivers these domain events, what does the orchestrator do?”

#### `MockOverlayRenderer`

- **Implements:** `OverlayRenderer`.
- **Mechanism:**
  - Records every `ShimmerRequest` in `play_calls: Arc<Mutex<Vec<...>>>`.
  - `with_delay(duration)` simulates a long-running play so `is_playing()` stays true (overlap tests).
  - `cancel()` sets a flag; `play()` returns `OverlayError::Cancelled` if interrupted.
  - Uses **std `thread::sleep`** inside `play()` (not Tokio timers) so `core` does not need a runtime dependency for the mock itself.

**Why this isolates GPU/windowing:** No winit window, no wgpu device, no shader. The orchestrator only sees port method calls and return values.

### Execution model for orchestrator tests

Orchestrator tests are `async` internally but often run from sync `#[test]` via a small helper:

1. Build `ShimmerOrchestrator::new(mock_power, mock_overlay, config)`.
2. `block_on(orchestrator.run().await)` using a **current-thread** Tokio runtime with time enabled.
3. Assert on `overlay.play_calls.len()` and trigger types.

Overlap and shutdown tests add **real OS threads** only to simulate concurrency between `run()` and `trigger_manual()` / `shutdown()` — still no platform adapters.

```
  Test thread                         Background thread
  ───────────                         ─────────────────
  spawn run().await ─────────────────► orchestrator.run()
  poll overlay.is_playing()
  trigger_manual().await
  join run thread
  assert play_calls
```

### Policy tests vs orchestrator tests

- **`should_auto_play` tests** are the smallest possible unit: one pure function, no ports, no async.
- **Orchestrator tests** prove wiring: polling the stream, calling policy, invoking `overlay.play().await`, overlap branches.

If a bug is “wrong rule,” fix policy tests first. If a bug is “rule never reached” or “play called twice,” fix orchestrator tests.

---

## 2. Adapter / Side-Effect Boundary Tests (`platform-linux`)

### What we are proving

`platform-linux` tests answer:

> “Does this adapter correctly translate messy OS signals into **stable domain outputs** (or stable internal backend behavior) without involving the orchestrator or GPU?”

They sit **below** the `PowerEventListener` port (for power) or **beside** real port impls (overlay session/shader), using **fakes** instead of production OS APIs where possible.

### Power: three layers of tests

```
  LinuxPowerListener  ──implements──►  PowerEventListener (core port)
         │
         uses
         ▼
  PowerSourceBackend trait  ◄──  MockPowerBackend (tests only)
         ▲
         │ also implemented by
  UpowerBackend | SysfsFallbackBackend
```

| Test target | Technique | Real OS? |
|-------------|-----------|----------|
| `LinuxPowerListener` debounce / transitions | `MockPowerBackend` in `listener.rs` tests | No |
| `SysfsFallbackBackend` parsing & notify | Temp dir under `/tmp`, fake `type`/`online` files | Fake sysfs tree only |
| `UpowerBackend` property reading | In-process **mock D-Bus** (`zbus` test server) | Session bus mock, not system UPower |
| `linux_power_smoke.rs` | Real `LinuxPowerBackend::select()` | Yes — **`#[ignore]`** |

### How we prove 400ms debounce coalescing (without waiting 400ms in CI)

Production default: `DEFAULT_DEBOUNCE = 400ms` in `LinuxPowerListener`.

The automated test `debounce_coalesces_rapid_flicker_into_single_transition` does **not** sleep 400ms. It uses:

```text
.with_debounce(Duration::from_millis(50))   // same algorithm, faster constant
```

**Why this is valid:** Debounce is implemented as generic duration logic — “quiet window after last change hint.” The test proves **coalescing behavior**, not the literal millisecond product constant. The production value is a separate configuration concern (documented in SPEC).

**Test story (execution steps):**

1. **Arrange — fake hardware flicker**
   - `MockPowerBackend::with_sequence(Battery, online=false, vec![true, false, true])`
   - A helper thread writes `online` and sends change hints on a channel **as fast as possible** (simulates rapid plug/unplug/plug).

2. **Act — subscribe to real listener**
   - `LinuxPowerListener::new(mock_backend).with_debounce(50ms)`
   - `listener.subscribe()` → real worker thread + real debounce loop (production code path).

3. **Assert — domain output**
   - Receive `InitialState { Battery }`.
   - Receive **one** `Transition { Battery → Ac }` (final settled state after quiet period).
   - `recv_timeout(100ms)` must **not** yield another transition (proves flicker did not create Battery→Ac→Battery→Ac spam).

```
  Flicker hints:  AC? ─┬─ off ─┬─ on
                      │       │
  Debounce window:    [─────── quiet 50ms ───────]
                      │
  Emitted Transition: Battery ───────────────► Ac  (once)
```

**What this test does NOT do:** It does not start `ShimmerOrchestrator`. Debounce is an adapter concern; orchestrator tests assume **already-normalized** `PowerEvent`s.

### Other platform-linux boundary patterns

- **Session controller** (`overlay/session.rs`): atomics + phase mutex — pure state machine, no GPU.
- **Shader module** (`overlay/shader.rs`): struct layout, embedded WGSL matches file on disk — no GPU required.
- **Sysfs tests**: inject `supply_root` path — never touch real `/sys/class/power_supply` in CI.
- **UPower unit tests**: mock bus objects; prove `read_upower_ac_online` when root `OnLine` is missing but `DisplayDevice.OnBattery` exists (modern UPower shape). Critical for future regressions when distros differ.

### Overlay: boundary vs smoke

| Test | Tier | Notes |
|------|------|-------|
| `wgpu_shimmer::cancel_when_idle_is_no_op` | Unit-ish | Port semantics without playing |
| `wgpu_shimmer` `#[ignore]` play/cancel tests | Manual GPU/X11 | Same as production renderer |
| `overlay_x11_smoke.rs` | Integration smoke | Full `play().await` — **`#[ignore]`** |

**Wayland (future):** Core orchestrator matrix unchanged. New work adds a **new overlay adapter** + boundary tests (fake compositor protocol or harness) + optional ignored smoke — same tier rules as X11 today.

---

## 3. Anatomy of a Perfect Pure Core Unit Test

Below is a **pseudo-code template** (not copy-paste production code) showing the shape every good `core` orchestrator test should follow.

### Template: Battery boot → plug AC → exactly one shimmer

```rust
// ─── ARRANGE (Given) ─────────────────────────────────────────────
let events = vec![
    PowerEvent::InitialState { source: Battery },
    PowerEvent::Transition { from: Battery, to: Ac },
];
let config = OrchestratorConfig::default();   // auto on, dry_run off, overlap Skip
let overlay = MockOverlayRenderer::new();     // completes play instantly

let orchestrator = ShimmerOrchestrator::new(
    MockPowerEventListener::new(events),      // fake power port
    overlay.clone(),                          // fake overlay port
    config,
);

// ─── ACT (When) ────────────────────────────────────────────────
let result = block_on(orchestrator.run().await);

// ─── ASSERT (Then) ─────────────────────────────────────────────
assert_eq!(overlay.play_calls.lock().unwrap().len(), 1);
assert_eq!(
    overlay.play_calls.lock().unwrap()[0].trigger,
    ShimmerTrigger::PowerTransition,
);
assert!(result is StreamEnded);  // mock closed stream after last event
```

### Line-by-line walkthrough (plain English)

| Lines | Meaning |
|-------|---------|
| `events = vec![...]` | **Given** the outside world reported these domain facts, in order. You control the story entirely. |
| `OrchestratorConfig::default()` | **Given** production-like policy unless the test is *about* toggling `auto_enabled`, `dry_run`, or `overlap_policy`. |
| `MockOverlayRenderer::new()` | **Given** a recorder that pretends to play shimmers without graphics. |
| `ShimmerOrchestrator::new(mock, mock, config)` | **When** we construct the real orchestrator — the unit under test — with fakes injected at the only two ports it knows. |
| `block_on(run().await)` | **When** we execute the real run loop: subscribe, poll stream, apply policy, maybe call `play`. |
| `play_calls.len() == 1` | **Then** the orchestrator decided to shimmer exactly once (not zero, not two). |
| `trigger == PowerTransition` | **Then** the reason was automatic power policy, not tray/CLI. |
| `StreamEnded` | **Then** the test ended because the mock power stream closed — expected for `MockPowerEventListener::new`, not a failure. |

### Variations you should recognize

| If the test is about… | Change in ARRANGE |
|------------------------|-------------------|
| Auto disabled | `config.auto_enabled = false` |
| Dry run | `config.dry_run = true` |
| Overlap Skip | `MockOverlayRenderer::with_delay(200ms)` + manual trigger while `is_playing()` |
| Overlap Restart | same + `overlap_policy: Restart` → expect 2 `play_calls` |
| Shutdown | `keep_alive_after_events(...)` + call `shutdown()` from another thread |

### Anti-patterns (not a “correct” core test)

- Importing `WgpuShimmerRenderer`, `LinuxPowerListener`, or `gtk` in `core` tests.
- Asserting on log lines instead of observable port behavior (`play_calls`, `is_playing`, return errors).
- Sleeping arbitrary seconds without using mock delay to model “still playing.”
- Testing debounce timing in `core` — that belongs in `platform-linux` with `MockPowerBackend`.

---

## 4. Auditing Rules for the User (3-point checklist)

When reviewing a proposed test in a PR, ask these three questions:

### ① Boundary: “Does this test live in the right crate?”

| If it asserts… | It belongs in… |
|----------------|----------------|
| Orchestrator/policy/overlap/shutdown | `core` (mocks only) |
| Debounce, sysfs parsing, UPower property mapping, session atomics, shader embed | `platform-linux` (fakes or temp files) |
| Real DISPLAY / GPU / system UPower | `platform-linux/tests` or `#[ignore]` — never required for CI green |

**Red flag:** `core` test that needs `DISPLAY`, D-Bus system bus, or spawns a real window.

**Red flag:** `platform-linux` test that duplicates the full SPEC orchestrator table — that’s duplicate coverage and couples adapter PRs to business rules.

### ② Coupling: “Is the test observing behavior, not implementation?”

**Good:** Assert `play_calls.len()`, `ShimmerTrigger`, `OverlayError::Cancelled`, emitted `PowerEvent` values.

**Bad:** Assert private fields, internal function call order, or “mock was called 3 times” on internal helpers the orchestrator does not expose.

**Bad:** Brittle time assertions (`sleep(400ms)` in core) when a fake backend + debounce parameter or `with_delay` mock expresses intent clearly.

**Good adapter test:** Rapid hint sequence → **one** `Transition` at the end.

**Bad adapter test:** Assert exact number of D-Bus signal handler invocations.

### ③ Complexity: “Is this the smallest test that proves the regression?”

**Prefer:**

- One scenario per test function (matches existing `orchestrator_test.rs` style).
- Pure function tests for pure policy.
- Fakes at the **lowest** trait that owns the behavior (`PowerSourceBackend` for debounce, not full UPower daemon).

**Push back when:**

- A single test checks five unrelated policies (hard to diagnose failures).
- CI-flaky tests depend on real hardware timing without `#[ignore]`.
- Test setup exceeds ~30 lines of plumbing — extract a fake builder or reuse existing mock patterns.

---

## How this supports future features (e.g. Wayland)

| Concern | Stays stable | Needs new tests |
|---------|--------------|-----------------|
| When to shimmer | `core` policy + orchestrator table | Only if product rules change |
| Power debounce / UPower quirks | Existing platform power tests | Extend if new signal sources appear |
| Overlay presentation | Port contract in `core` | New adapter + boundary tests + ignored smoke |
| CI green bar | Mocks + fakes | Do not move orchestrator table to GPU CI |

**Review heuristic for Wayland PRs:** You should see new `platform-linux` overlay tests and smokes, **zero** changes to the meaning of `orchestrator_test.rs` unless product behavior intentionally changes.

---

## Quick reference: commands

| Goal | Command |
|------|---------|
| CI-equivalent suite | `make test` |
| Include ignored smokes | `cargo test --workspace -- --ignored` |
| X11 overlay smoke only | `cargo test -p power-shimmer-platform-linux overlay_x11 -- --ignored --nocapture` |

---

## Summary

- **`core`** tests are an in-memory **behavior contract** using `MockPowerEventListener` and `MockOverlayRenderer` — no OS, no GPU.
- **`platform-linux`** tests prove **translation and stability** (debounce, sysfs, UPower shapes) using fakes one layer below or beside real ports.
- **Smokes** prove “it works on a real machine” and are explicitly **`#[ignore]`** so PR CI stays trustworthy.
- Your audit lens: **right crate**, **observe ports/domain**, **smallest proof**.
