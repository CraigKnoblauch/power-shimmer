//! Linux overlay rendering adapters.

pub mod placement;

pub use placement::window_covers_monitor;
mod render_loop;
mod session;
mod shader;
pub mod window_placement;
pub mod wgpu_shimmer;
pub mod x11_click_through;

pub use render_loop::require_x11_session;
pub use window_placement::{LinuxWindowPlacementProbe, probe_primary_window_placement};
pub use wgpu_shimmer::WgpuShimmerRenderer;

#[cfg(feature = "wayland")]
pub mod wayland_layer_shell;
