//! Manual X11 overlay smoke test — requires DISPLAY and a compositor.

use std::time::Duration;

use power_shimmer_core::ports::OverlayRenderer;
use power_shimmer_core::{ShimmerConfig, ShimmerRequest, ShimmerTrigger};
use power_shimmer_platform_linux::WgpuShimmerRenderer;

#[tokio::test]
#[ignore = "requires X11 DISPLAY and GPU; run: cargo test -p power-shimmer-platform-linux overlay_x11 -- --ignored --nocapture"]
async fn overlay_x11_smoke_plays_and_finishes() {
    let renderer = WgpuShimmerRenderer::new();
    let request = ShimmerRequest {
        config: ShimmerConfig {
            duration_ms: 500,
            ..ShimmerConfig::default()
        },
        trigger: ShimmerTrigger::Manual,
    };

    assert!(!renderer.is_playing());
    renderer.play(request).await.expect("play shimmer");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!renderer.is_playing());
}
