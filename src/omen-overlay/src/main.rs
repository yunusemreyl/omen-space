use gtk::prelude::*;
use libadwaita as adw;

mod daemon_client;
mod overlay_window;
mod i18n;

const APP_ID: &str = "org.hp.omen.Overlay";

/// Older omen-gui / omen-cli builds started the overlay as `omen-overlay --daemon`.
/// The overlay has no background mode (it is opened on demand by omen-tray), and
/// GApplication would reject the unknown option and exit anyway, so such a launch
/// is turned into a clean, silent no-op instead of an error or a stray window.
fn is_legacy_daemon_launch<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().skip(1).any(|a| a.as_ref() == "--daemon")
}

fn main() {
    if is_legacy_daemon_launch(std::env::args()) {
        eprintln!(
            "omen-overlay: --daemon is no longer supported; the HUD is opened on demand \
             (Shift+F2 or `omen-cli overlay toggle`)."
        );
        return;
    }

    // Prefer Wayland, but fall back to X11 instead of panicking on an X11-only session.
    // An explicit GDK_BACKEND from the user's environment always wins.
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "wayland,x11");
    }
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,omen_overlay=info"),
    )
    .init();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|_| {
        adw::init().expect("Failed to initialize libadwaita");
        let provider = gtk::CssProvider::new();
        provider.load_from_string(include_str!("style.css"));
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });

    app.connect_activate(|app| {

        let overlay = overlay_window::OverlayWindow::new(app);
        
        let key_controller = gtk::EventControllerKey::new();
        let app_c = app.clone();
        key_controller.connect_key_pressed(move |_, key, _keycode, _state| {
            let key_val = key.name().unwrap_or_default().to_lowercase();
            // Only Escape closes the HUD from inside. Shift+F2 is the global toggle handled
            // by omen-space-daemon -> omen-tray; also quitting on F2 here raced with that
            // path (the overlay closed itself, then the tray saw nothing running and
            // opened a new one, so the HUD could never be closed with the hotkey).
            if key_val == "escape" {
                app_c.quit();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        overlay.window.add_controller(key_controller);

        let app_c2 = app.clone();
        overlay.window.connect_close_request(move |_| {
            app_c2.quit();
            glib::Propagation::Stop
        });

        let win = overlay.window.clone();
        let overlay_c = overlay.clone();
        daemon_client::subscribe_telemetry(move |stats| {
            if win.is_visible() {
                overlay_c.update_telemetry(&stats);
            }
        });

        overlay.window.present();
    });

    app.run();
}

#[cfg(test)]
mod tests {
    use super::is_legacy_daemon_launch;

    #[test]
    fn legacy_daemon_flag_is_detected() {
        assert!(is_legacy_daemon_launch(["omen-overlay", "--daemon"]));
        assert!(is_legacy_daemon_launch(["/usr/bin/omen-overlay", "--other", "--daemon"]));
    }

    #[test]
    fn normal_launches_are_not_treated_as_legacy() {
        assert!(!is_legacy_daemon_launch(["omen-overlay"]));
        assert!(!is_legacy_daemon_launch(["omen-overlay", "--help"]));
        // The program name itself (argv[0]) must never count as the flag.
        assert!(!is_legacy_daemon_launch(["--daemon"]));
    }
}
