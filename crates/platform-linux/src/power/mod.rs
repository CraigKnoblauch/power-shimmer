//! Linux power event adapters.

pub mod backend;
pub mod listener;
pub mod sysfs_fallback;
pub mod upower;

pub use backend::{source_from_online, PowerSourceBackend};
pub use listener::LinuxPowerListener;
