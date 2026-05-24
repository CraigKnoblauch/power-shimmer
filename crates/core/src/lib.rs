//! Power Shimmer core: domain types, port traits, and application services.
//!
//! This crate has no platform or rendering dependencies. See [`SPEC.md`](../../SPEC.md).

pub mod domain;
pub mod ports;
pub mod services;

/// Test doubles for port traits. Not used in production wiring.
#[doc(hidden)]
pub mod testing;

pub use domain::{
    MonitorTarget, OrchestratorConfig, OrchestratorError, OverlapPolicy, OverlayError, PowerEvent,
    PowerListenerError, PowerSource, ShimmerConfig, ShimmerRequest, ShimmerTrigger,
};
pub use ports::{OverlayRenderer, PowerEventListener, PowerEventStream, StreamRecvResult};
pub use services::{should_auto_play, ShimmerOrchestrator};
pub use testing::{MockOverlayRenderer, MockPowerEventListener};
