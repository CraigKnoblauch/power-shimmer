//! Power Shimmer application crate (composition root).

pub mod cli;
pub mod config;
pub mod error;
pub mod logging;

#[cfg(all(feature = "linux", feature = "tray"))]
pub mod tray;

#[cfg(feature = "linux")]
pub mod wiring;
