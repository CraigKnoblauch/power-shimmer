//! Injectable power source backends (`UPower`, sysfs, test doubles).

use std::time::Duration;

use power_shimmer_core::PowerSource;

/// OS-neutral power readings used by [`super::listener::LinuxPowerListener`].
pub trait PowerSourceBackend: Send + Sync {
    /// Active power source when a subscription begins.
    fn initial_source(&self) -> PowerSource;

    /// Blocks until the backend believes online state may have changed.
    ///
    /// Returns `None` when the backend is shutting down.
    fn wait_online_change(&self) -> Option<()>;

    /// Current settled AC-online flag. `None` when still unknown.
    fn read_online(&self) -> Option<bool>;

    /// Waits up to `timeout` for another change hint; `None` on timeout.
    fn try_wait_online_change(&self, timeout: Duration) -> Option<()>;
}

/// Maps UPower/sysfs `online` flag to domain power source.
#[must_use]
pub fn source_from_online(online: bool) -> PowerSource {
    if online {
        PowerSource::Ac
    } else {
        PowerSource::Battery
    }
}

/// Maps an optional online reading to domain power source.
#[must_use]
pub fn source_from_online_option(online: Option<bool>) -> PowerSource {
    match online {
        Some(value) => source_from_online(value),
        None => PowerSource::Unknown,
    }
}
