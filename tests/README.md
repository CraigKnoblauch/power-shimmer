# Tests

Integration and unit tests live next to the crates that exercise them:

| Path | Purpose |
|---|---|
| `crates/core/tests/orchestrator_test.rs` | Orchestrator unit tests (SPEC.md) |
| `crates/platform-linux/tests/linux_power_smoke.rs` | UPower smoke test (`#[ignore]` until adapter exists) |

Run the full suite from the repository root:

```bash
./scripts/test.sh
```
