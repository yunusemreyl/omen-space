use gtk::prelude::*;
use libadwaita as adw;
use clap::Parser;
use std::rc::Rc;
use std::cell::RefCell;

mod daemon_client;
mod overlay_window;

const APP_ID: &str = "org.hp.omen.Overlay";

#[derive(Parser, Debug)]
#[command(name = "omen-overlay", about = "OMEN Space Shift+F2 Quick HUD Overlay")]
struct Args {
    /// Toggle overlay visibility
    #[arg(short, long)]
    toggle: bool,

    /// Show overlay
    #[arg(long)]
    show: bool,

    /// Hide overlay
    #[arg(long)]
    hide: bool,

    /// Start in background daemon mode (hidden until Shift+F2 is pressed)
    #[arg(short, long)]
    daemon: bool,
}

fn acquire_single_instance_lock() -> Option<std::fs::File> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/tmp/user-{}", unsafe { libc::getuid() }));
    let lock_path = format!("{}/omen-overlay.lock", runtime_dir);

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .ok()?;

    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if res != 0 {
        return None;
    }

    Some(file)
}

fn main() {
    std::env::set_var("GDK_BACKEND", "wayland,x11");
    env_logger::init();
    let args = Args::parse();

    // If --toggle, --show, or --hide was requested, send D-Bus signal and exit
    if args.toggle || args.show || args.hide {
        let rt = daemon_client::get_runtime();
        rt.block_on(async {
            let _ = daemon_client::send_toggle_overlay_signal().await;
        });
        return;
    }

    let _lock = match acquire_single_instance_lock() {
        Some(file) => file,
        None => {
            // Another instance already running, trigger toggle on it
            let rt = daemon_client::get_runtime();
            rt.block_on(async {
                let _ = daemon_client::send_toggle_overlay_signal().await;
            });
            return;
        }
    };

    let rt = daemon_client::get_runtime();
    let _guard = rt.enter();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let overlay_ref: Rc<RefCell<Option<Rc<overlay_window::OverlayWindow>>>> = Rc::new(RefCell::new(None));

    app.connect_startup({
        let overlay_ref = overlay_ref.clone();
        move |app| {
            // Keep the application held alive in memory indefinitely even when 0 windows are visible
            std::mem::forget(app.hold());
            adw::init().expect("Failed to initialize libadwaita");

            let provider = gtk::CssProvider::new();
            provider.load_from_string(include_str!("style.css"));
            gtk::style_context_add_provider_for_display(
                &gtk::gdk::Display::default().unwrap(),
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            let overlay = overlay_window::OverlayWindow::new();
            *overlay_ref.borrow_mut() = Some(overlay.clone());

            // Listen for Shift+F2 / D-Bus macro key overlay signals
            let o_hotkey = overlay.clone();
            daemon_client::subscribe_hotkey(move |key| {
                if key == "overlay" {
                    o_hotkey.toggle_visibility();
                }
            });

            // Listen for live system telemetry updates
            let o_telem = overlay.clone();
            daemon_client::subscribe_telemetry(move |stats| {
                if o_telem.window.is_visible() {
                    o_telem.update_telemetry(&stats);
                }
            });
        }
    });

    let daemon_mode = args.daemon;
    app.connect_activate(move |_| {
        if let Some(overlay) = overlay_ref.borrow().as_ref() {
            if !daemon_mode {
                overlay.toggle_visibility();
            }
        }
    });

    app.run_with_args(&Vec::<String>::new());
}
