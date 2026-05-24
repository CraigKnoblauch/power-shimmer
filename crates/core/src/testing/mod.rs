//! Test doubles for port traits. Not used in production wiring.

mod mock_overlay;
mod mock_power;

pub use mock_overlay::MockOverlayRenderer;
pub use mock_power::MockPowerEventListener;
