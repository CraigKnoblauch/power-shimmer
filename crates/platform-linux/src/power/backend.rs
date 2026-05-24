//! Injectable power source backends (UPower, sysfs, test doubles).

use power_shimmer_core::PowerSource;

/// OS-neutral power readings used by [`super::listener::LinuxPowerListener`].
pub trait PowerSourceBackend: Send + Sync {
    /// Active power source when a subscription begins.
    fn initial_source(&self) -> PowerSource;

    /// Blocks until the AC online flag changes, or returns `None` when closed.
    fn wait_online_change(&self) -> Option<bool>;
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
