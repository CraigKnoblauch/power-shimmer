//! Primary-monitor fullscreen placement contract (shared harness).
//!
//! CI runs this under Xvfb; locally use a real or virtual X11 display (`DISPLAY` set).

use power_shimmer_overlay_contract::{
    ContractConfig, run_primary_fullscreen_contract,
};
use power_shimmer_platform_linux::WgpuShimmerRenderer;

#[tokio::test]
#[ignore = "requires X11 DISPLAY and GPU; CI runs under Xvfb: cargo test -p power-shimmer-platform-linux overlay_placement_contract -- --ignored --nocapture"]
async fn overlay_placement_contract_primary_fullscreen() {
    let renderer = WgpuShimmerRenderer::new();
    run_primary_fullscreen_contract(renderer, ContractConfig::default())
        .await
        .expect("overlay placement contract");
}
