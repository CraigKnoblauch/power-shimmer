//! Port traits (interfaces) for platform adapters.

pub mod overlay;
pub mod power;

pub use power::{PowerEventListener, PowerEventStream};
