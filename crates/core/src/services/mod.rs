//! Application services that coordinate port implementations.

pub mod policy;
pub mod shimmer_orchestrator;

pub use policy::should_auto_play;
pub use shimmer_orchestrator::ShimmerOrchestrator;
