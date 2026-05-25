//! Window-only primary fullscreen placement (winit + Xvfb, no GPU).
//!
//! CI runs this in the Xvfb PR gate without Mesa/GL.

use power_shimmer_overlay_contract::run_primary_window_placement_contract;
use power_shimmer_platform_linux::overlay::LinuxWindowPlacementProbe;

#[test]
#[ignore = "requires X11 DISPLAY (no GPU); CI uses Xvfb: cargo test -p power-shimmer-platform-linux overlay_window_placement_contract -- --ignored --nocapture"]
fn overlay_window_placement_contract_primary() {
    run_primary_window_placement_contract(LinuxWindowPlacementProbe)
        .expect("window placement contract");
}
