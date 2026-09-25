use gtk::prelude::*;
use libadwaita as adw;

mod daemon_client;
mod overlay_window;
mod i18n;

const APP_ID: &str = "org.hp.omen.Overlay";

fn main() {
    std::env::set_var("GDK_BACKEND", "wayland");
    env_logger::init();
    
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
            if key_val == "escape" || key_val == "f2" {
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
