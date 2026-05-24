# Issue: UPower `OnLine` property unavailable on current UPower version

**Status:** Identified — not yet fixed  
**Severity:** Medium (functional fallback exists; primary path unused)  
**Affected component:** `platform-linux` UPower adapter  
**Fallback:** sysfs `/sys/class/power_supply` (working)

---

## Identification

### Symptom

At startup the UPower adapter logs:

```
WARN power_shimmer_platform_linux::power::upower: failed to read UPower OnLine property
  error=org.freedesktop.DBus.Error.InvalidArgs: No such property "OnLine"
```

After a 500 ms retry window, backend selection falls back to sysfs:

```
DEBUG power_shimmer_platform_linux::power: UPower unavailable; selecting sysfs fallback backend
```

Subsequent plug/unplug events are detected via sysfs polling (confirmed: `sysfs AC online state changed last=Some(false) current=Some(true)`).

### Root cause

`UpowerBackend` reads the `OnLine` property on `org.freedesktop.UPower` at `/org/freedesktop/UPower`:

```rust
properties.get(interface, "OnLine").await
```

On the reporter's system (and likely UPower ≥ 0.99.x), this property is not exposed on the root UPower object. The D-Bus service is reachable, but `read_upower_online()` returns `None`, so `is_available()` never becomes true:

```rust
pub fn is_available(&self) -> bool {
    self.state.connected.load(Ordering::SeqCst) && self.read_online().is_some()
}
```

Backend selection then correctly chooses sysfs after the wait window in `LinuxPowerBackend::select()`.

### Impact

- **Functional:** Low when sysfs is readable (typical laptops). Battery → AC detection works via fallback.
- **Spec intent:** Medium. SPEC designates UPower as the Linux v1 primary adapter; sysfs is auto-selected fallback, not the preferred path.
- **Latency:** Up to 500 ms extra at startup before sysfs is selected.
- **Headless/embedded:** Sysfs may be the only option anyway; no regression there.

This issue does **not** cause the daemon crash. That is tracked separately in `tray-gtk-init-panic.md`.

---

## Planned resolution

All changes confined to `platform-linux` UPower adapter. Event protocol and debounce behavior unchanged.

### 1. Read AC state from supported UPower interfaces

Replace or supplement root `OnLine` with properties available on the target UPower version, in priority order:

1. **`DisplayDevice`** — `OnBattery` on `/org/freedesktop/UPower/devices/DisplayDevice` (invert: `OnBattery == false` → AC).
2. **Line-power devices** — enumerate `/org/freedesktop/UPower/devices/*`, match `Type == Line Power` (or equivalent enum), read per-device `Online`.
3. **Root `OnLine`** — retain as legacy fallback for older UPower installs.

Map all readings to the existing internal `Option<bool>` online state; downstream `LinuxPowerListener` and orchestrator logic stay unchanged.

### 2. Subscribe to the correct property change signals

Update `receive_properties_changed()` handling to react to `OnBattery` / device `Online` changes, not only root `OnLine`.

### 3. Availability check

Revise `is_available()` to return true when any supported read path yields `Some(bool)`, not only when legacy `OnLine` succeeds.

### 4. Tests

- Unit tests with mocked property responses for DisplayDevice and legacy paths.
- Existing orchestrator and listener tests remain unchanged (they use mock backends).
- Optional integration smoke test gated on D-Bus (similar to existing `linux_power_smoke`).

### 5. Keep sysfs fallback

Do not remove `SysfsFallbackBackend`. SPEC requires auto-selection when D-Bus/UPower is unavailable; sysfs remains the fallback when all UPower read paths fail.

### Verification

| Test | Expected |
|------|----------|
| Machine with modern UPower | `selected UPower backend` log; no `OnLine` warning |
| Machine without UPower / D-Bus | sysfs fallback (unchanged) |
| Battery → AC on UPower path | `Transition { from: Battery, to: Ac }` after 400 ms debounce |
| `cargo test -p power-shimmer-platform-linux` | Pass |

---

## SPEC.md compliance

### Does this issue violate SPEC?

**Partially — adapter obligation gap, not an architectural violation.**

SPEC § Module 2 — Adapter obligations (Linux v1 — UPower):

| Obligation | Current state |
|------------|---------------|
| Read `Online` / device `Type` from UPower D-Bus | **Partial:** reads root `OnLine` only; does not use DisplayDevice or enumerated line-power devices |
| Emit `InitialState` then debounced `Transition` | **Met** (via sysfs fallback when UPower read fails) |
| Map `Online == true` → `Ac`, `false` → `Battery` | **Met** once online bool is obtained |
| 400 ms debounce | **Met** |
| Unknown at startup if UPower unavailable; retry in background | **Met** — UPower monitor retries; sysfs selected for immediate use |

The adapter does not leak D-Bus types across boundaries and does not encode trigger policy. The gap is **incomplete use of the UPower surface**, causing unnecessary fallback to sysfs on systems where UPower is actually available.

### Does the planned fix violate SPEC?

**No.** The fix fulfills existing adapter obligations more completely:

| SPEC rule | Fix alignment |
|-----------|---------------|
| Power Listener emits normalized `PowerEvent` only | Unchanged — same `InitialState` / `Transition` protocol |
| Adapters translate, not decide | Unchanged — still maps OS signals to `PowerSource`; Battery → AC policy stays in Orchestrator |
| Debounce 400 ms | Unchanged |
| sysfs fallback when D-Bus unavailable | Preserved |
| Power Listener must NOT call Overlay Renderer | Unchanged |
| No D-Bus/zbus types in public `core` signatures | Unchanged — all D-Bus code stays inside `platform-linux` |

### SPEC sections satisfied by the fix

- **Adapter obligations (Linux v1 — UPower):** Startup read and change subscription from correct UPower properties.
- **Adapter obligations (Linux fallback — sysfs):** Unchanged; remains auto-selected when UPower truly unavailable.

No revision to SPEC.md is required. The spec already allows reading device `Type` and `Online`; the fix aligns implementation with that text.
