//! Domain value types and errors (pure data, no I/O).

pub mod config;
pub mod errors;
pub mod events;

pub use config::{OrchestratorConfig, OverlapPolicy};
pub use errors::{OrchestratorError, OverlayError, PowerListenerError};
pub use events::{
    MonitorTarget, PowerEvent, PowerSource, ShimmerConfig, ShimmerRequest, ShimmerTrigger,
};
