//! Linux power event adapters.

pub mod backend;
pub mod listener;
pub mod sysfs_fallback;
pub mod upower;

pub use backend::{source_from_online, source_from_online_option, PowerSourceBackend};
pub use listener::LinuxPowerListener;
pub use sysfs_fallback::SysfsFallbackBackend;
pub use upower::UpowerBackend;

use tracing::debug;

/// Production backend: `UPower` when available, otherwise sysfs fallback.
pub enum LinuxPowerBackend {
    /// Primary D-Bus `UPower` adapter.
    Upower(UpowerBackend),
    /// sysfs `/sys/class/power_supply` poll fallback.
    Sysfs(SysfsFallbackBackend),
}

impl LinuxPowerBackend {
    /// Selects `UPower` when D-Bus is reachable; otherwise sysfs fallback.
    #[must_use]
    pub fn select() -> Self {
        let upower = UpowerBackend::new();
        if wait_for_upower(&upower) {
            debug!("selected UPower backend");
            Self::Upower(upower)
        } else {
            debug!("UPower unavailable; selecting sysfs fallback backend");
            Self::Sysfs(SysfsFallbackBackend::new())
        }
    }
}

impl PowerSourceBackend for LinuxPowerBackend {
    fn initial_source(&self) -> power_shimmer_core::PowerSource {
        match self {
            Self::Upower(backend) => backend.initial_source(),
            Self::Sysfs(backend) => backend.initial_source(),
        }
    }

    fn wait_online_change(&self) -> Option<()> {
        match self {
            Self::Upower(backend) => backend.wait_online_change(),
            Self::Sysfs(backend) => backend.wait_online_change(),
        }
    }

    fn read_online(&self) -> Option<bool> {
        match self {
            Self::Upower(backend) => backend.read_online(),
            Self::Sysfs(backend) => backend.read_online(),
        }
    }

    fn try_wait_online_change(&self, timeout: std::time::Duration) -> Option<()> {
        match self {
            Self::Upower(backend) => backend.try_wait_online_change(timeout),
            Self::Sysfs(backend) => backend.try_wait_online_change(timeout),
        }
    }
}

fn wait_for_upower(backend: &UpowerBackend) -> bool {
    if backend.is_available() {
        return true;
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < deadline {
        if backend.is_available() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    backend.is_available()
}
