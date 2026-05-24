//! Tray GTK initialization tests (see notes/issues/tray-gtk-init-panic.md).

use power_shimmer_app::tray::{init_tray_gtk, prepare_tray_menu};

/// [`init_tray_gtk`] must call `gtk::init()` on the tray thread before any menu is built.
///
/// Fails while `init_tray_gtk` is a stub that skips `gtk::init()`, reproducing the production
/// panic: `GTK has not been initialized. Call gtk::init first.`
#[test]
fn init_tray_gtk_initializes_gtk_on_fresh_thread() {
    std::thread::Builder::new()
        .name("tray-gtk-test".into())
        .spawn(|| {
            assert!(
                !gtk::is_initialized(),
                "test setup: GTK must start uninitialized on a fresh thread"
            );

            init_tray_gtk().expect("init_tray_gtk should return Ok");

            assert!(
                gtk::is_initialized(),
                "init_tray_gtk must call gtk::init() on the tray thread \
                 (see notes/issues/tray-gtk-init-panic.md)"
            );

            prepare_tray_menu(true).expect("prepare_tray_menu should succeed after gtk init");
        })
        .expect("spawn tray test thread")
        .join()
        .expect("tray test thread joined");
}
