//! Power Shimmer core: domain types, port traits, and application services.
//!
//! This crate has no platform or rendering dependencies. See [`SPEC.md`](../../SPEC.md).

pub mod domain;
pub mod ports;
pub mod services;

pub use domain::{
    MonitorTarget, OrchestratorError, OverlayError, PowerEvent, PowerListenerError, PowerSource,
    ShimmerConfig, ShimmerRequest, ShimmerTrigger,
};
pub use ports::{PowerEventListener, PowerEventStream};
