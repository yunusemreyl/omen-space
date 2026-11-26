use gtk::prelude::*;
use libadwaita as adw;

mod daemon_client;
mod i18n;
mod overlay_window;

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

fn apply_appearance_mode() {
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(am) = json.get("appearance_mode").and_then(|v| v.as_u64()) {
                    let scheme = match am {
                        1 => adw::ColorScheme::ForceLight,
                        2 => adw::ColorScheme::ForceDark,
                        _ => adw::ColorScheme::Default,
                    };
                    adw::StyleManager::default().set_color_scheme(scheme);
                }
            }
        }
    }
}

fn main() {
    if is_legacy_daemon_launch(std::env::args()) {
        eprintln!(
            "omen-overlay: --daemon is no longer supported; the HUD is opened on demand \
             (Shift+F2 or `omen-cli overlay toggle`)."
        );
        return;
    }

    omen_types::repair_graphical_session_env();

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
        apply_appearance_mode();
        if let Some(display) = gtk::gdk::Display::default() {
            let icon_theme = gtk::IconTheme::for_display(&display);
            icon_theme.add_search_path("assets");
            icon_theme.add_search_path("/usr/share/omen-space/assets");

            let provider = gtk::CssProvider::new();
            provider.load_from_string(include_str!("style.css"));
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
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
        assert!(is_legacy_daemon_launch([
            "/usr/bin/omen-overlay",
            "--other",
            "--daemon"
        ]));
    }

    #[test]
    fn normal_launches_are_not_treated_as_legacy() {
        assert!(!is_legacy_daemon_launch(["omen-overlay"]));
        assert!(!is_legacy_daemon_launch(["omen-overlay", "--help"]));
        // The program name itself (argv[0]) must never count as the flag.
        assert!(!is_legacy_daemon_launch(["--daemon"]));
    }
}
