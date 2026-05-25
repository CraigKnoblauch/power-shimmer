//! Linux overlay rendering adapters.

mod overlay_hint_policy;
mod render_loop;
mod session;
mod shader;
pub mod wgpu_shimmer;
pub mod x11_click_through;

pub use wgpu_shimmer::WgpuShimmerRenderer;

#[cfg(feature = "wayland")]
pub mod wayland_layer_shell;
