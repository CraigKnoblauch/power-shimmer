//! Linux platform adapters for power events and overlay rendering.

pub mod overlay;
pub mod power;

pub use overlay::WgpuShimmerRenderer;
