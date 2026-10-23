use gtk::gio;
use gtk::prelude::*;
use libadwaita as adw;
use std::cell::{Cell, RefCell};

/* ─────────────────────────────────────────────────────────────
   ui_scale.rs — Interface zoom (Ctrl+Shift+Plus / Ctrl+Shift+Minus)

   Enlarges the whole pane instead of going fullscreen, so the UI stays
   readable on large/high-density monitors. Three things move together:
     1. the bundled stylesheet is re-emitted with every `px` length
        multiplied by the zoom factor (cards, paddings, font sizes),
     2. the app-local font DPI is scaled the same way so widgets that
        use the theme font (Adw rows, plain labels) follow,
     3. the window grows proportionally, clamped to the monitor it is on.

   The factor is persisted in ~/.config/omenspace/settings.json so it
   survives a restart.
   ───────────────────────────────────────────────────────────── */

pub const MIN_SCALE: f64 = 0.8;
pub const MAX_SCALE: f64 = 2.0;
pub const SCALE_STEP: f64 = 0.1;

/// Window size at 100 %, kept in sync with the builder in `build_ui`.
const BASE_WIDTH: i32 = 1150;
const BASE_HEIGHT: i32 = 820;
/// GTK stores `gtk-xft-dpi` in 1024ths of a dot per inch.
const FALLBACK_XFT_DPI: i32 = 96 * 1024;

thread_local! {
    static SCALE: Cell<f64> = const { Cell::new(1.0) };
    static BASE_XFT_DPI: Cell<i32> = const { Cell::new(0) };
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<adw::ApplicationWindow>> = const { RefCell::new(None) };
    static SCALE_SPIN: RefCell<Option<gtk::SpinButton>> = const { RefCell::new(None) };
    static SYNCING_SPIN: Cell<bool> = const { Cell::new(false) };
}

/// Current zoom factor (1.0 = 100 %).
pub fn scale() -> f64 {
    SCALE.with(|s| s.get())
}

fn clamp(factor: f64) -> f64 {
    ((factor * 100.0).round() / 100.0).clamp(MIN_SCALE, MAX_SCALE)
}

/// Remember the window the zoom applies to and restore the saved factor.
pub fn init(window: &adw::ApplicationWindow) {
    WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
    let saved = load_saved_scale();
    if (saved - 1.0).abs() > f64::EPSILON {
        apply(saved);
    }
}

/// Register the app-wide zoom actions and their accelerators.
pub fn install_shortcuts(app: &adw::Application) {
    let action_in = gio::SimpleAction::new("zoom-in", None);
    action_in.connect_activate(|_, _| apply(scale() + SCALE_STEP));
    app.add_action(&action_in);

    let action_out = gio::SimpleAction::new("zoom-out", None);
    action_out.connect_activate(|_, _| apply(scale() - SCALE_STEP));
    app.add_action(&action_out);

    let action_reset = gio::SimpleAction::new("zoom-reset", None);
    action_reset.connect_activate(|_, _| apply(1.0));
    app.add_action(&action_reset);

    // Several spellings per action on purpose: "+" and "-" land on different
    // keyvals depending on the layout and on whether Shift is held, and the
    // numeric keypad reports its own keyvals.
    app.set_accels_for_action(
        "app.zoom-in",
        &[
            "<Ctrl><Shift>plus",
            "<Ctrl>plus",
            "<Ctrl><Shift>equal",
            "<Ctrl>equal",
            "<Ctrl><Shift>KP_Add",
            "<Ctrl>KP_Add",
        ],
    );
    app.set_accels_for_action(
        "app.zoom-out",
        &[
            "<Ctrl><Shift>minus",
            "<Ctrl>minus",
            "<Ctrl><Shift>underscore",
            "<Ctrl><Shift>KP_Subtract",
            "<Ctrl>KP_Subtract",
        ],
    );
    app.set_accels_for_action("app.zoom-reset", &["<Ctrl><Shift>0", "<Ctrl>0", "<Ctrl>KP_0"]);
}

/// Keep the Settings page spin button in sync with keyboard zooming.
pub fn bind_spin(spin: &gtk::SpinButton) {
    SYNCING_SPIN.with(|flag| flag.set(true));
    spin.set_value(scale() * 100.0);
    SYNCING_SPIN.with(|flag| flag.set(false));
    SCALE_SPIN.with(|s| *s.borrow_mut() = Some(spin.clone()));
}

/// True while `apply` is writing the spin button's value back.
pub fn is_syncing_spin() -> bool {
    SYNCING_SPIN.with(|flag| flag.get())
}

/// Apply `factor` (clamped) to the stylesheet, the font DPI and the window.
pub fn apply(factor: f64) {
    let factor = clamp(factor);
    SCALE.with(|s| s.set(factor));

    apply_css(factor);
    apply_font_dpi(factor);
    resize_window(factor);

    SCALE_SPIN.with(|s| {
        if let Some(spin) = s.borrow().as_ref() {
            // Guarded: set_value() emits ::value-changed, which would call
            // straight back into apply().
            SYNCING_SPIN.with(|flag| flag.set(true));
            spin.set_value(factor * 100.0);
            SYNCING_SPIN.with(|flag| flag.set(false));
        }
    });

    save_scale(factor);
}

fn apply_css(factor: f64) {
    let provider = PROVIDER.with(|p| {
        let mut slot = p.borrow_mut();
        if slot.is_none() {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                // One notch above the base stylesheet so the scaled copy wins.
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            *slot = Some(provider);
        }
        slot.clone()
    });

    let Some(provider) = provider else { return };

    if (factor - 1.0).abs() < f64::EPSILON {
        // Nothing to override: fall back to the untouched stylesheet.
        provider.load_from_string("");
    } else {
        provider.load_from_string(&scale_px_lengths(include_str!("style.css"), factor));
    }
}

fn apply_font_dpi(factor: f64) {
    let Some(settings) = gtk::Settings::default() else { return };

    let base = BASE_XFT_DPI.with(|b| {
        if b.get() == 0 {
            // Capture the session's own DPI once, so zooming stays relative to
            // whatever text scaling the desktop already applies.
            let current = settings.gtk_xft_dpi();
            b.set(if current > 0 { current } else { FALLBACK_XFT_DPI });
        }
        b.get()
    });

    settings.set_gtk_xft_dpi((base as f64 * factor).round() as i32);
}

fn resize_window(factor: f64) {
    WINDOW.with(|w| {
        let Some(window) = w.borrow().clone() else { return };
        // A window the user maximized (or a compositor did) stays where it is;
        // only the contents scale.
        if window.is_maximized() || window.is_fullscreen() {
            return;
        }

        let mut width = (BASE_WIDTH as f64 * factor).round() as i32;
        let mut height = (BASE_HEIGHT as f64 * factor).round() as i32;
        if let Some((max_width, max_height)) = monitor_limits(&window) {
            width = width.min(max_width);
            height = height.min(max_height);
        }
        window.set_default_size(width, height);
    });
}

/// Largest window size allowed on the monitor the window sits on. Before the
/// window is realized there is no surface to ask, so the first monitor of the
/// display stands in — enough to keep a factor restored from disk from asking
/// for a window taller than the screen.
fn monitor_limits(window: &adw::ApplicationWindow) -> Option<(i32, i32)> {
    let display = gtk::gdk::Display::default()?;
    let monitor = if window.is_realized() {
        display.monitor_at_surface(&window.surface())
    } else {
        display
            .monitors()
            .item(0)
            .and_downcast::<gtk::gdk::Monitor>()
    }?;
    let geometry = monitor.geometry();

    // Stay clear of the whole monitor: a window sized to ~95 % of the screen is
    // auto-maximized by Mutter, and a maximized window is then pinned there
    // (zooming out could no longer shrink it), which is the opposite of what
    // this feature is for. 85 % still fills a large monitor comfortably.
    const MAX_MONITOR_RATIO: f64 = 0.85;
    Some((
        (geometry.width() as f64 * MAX_MONITOR_RATIO) as i32,
        (geometry.height() as f64 * MAX_MONITOR_RATIO) as i32,
    ))
}

/// Multiply every `px` length in `css` by `factor`, leaving the rest alone.
fn scale_px_lengths(css: &str, factor: f64) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + css.len() / 8);
    let mut i = 0;

    while i < css.len() {
        let c = css[i..].chars().next().unwrap();
        let starts_number = c.is_ascii_digit()
            || (c == '.' && matches!(bytes.get(i + 1), Some(b'0'..=b'9')));

        if starts_number {
            let mut j = i;
            while matches!(bytes.get(j), Some(b'0'..=b'9') | Some(b'.')) {
                j += 1;
            }
            if css[j..].starts_with("px") {
                if let Ok(value) = css[i..j].parse::<f64>() {
                    out.push_str(&format_length(value * factor));
                    out.push_str("px");
                    i = j + 2;
                    continue;
                }
            }
            out.push_str(&css[i..j]);
            i = j;
            continue;
        }

        out.push(c);
        i += c.len_utf8();
    }

    out
}

fn format_length(value: f64) -> String {
    let mut text = format!("{:.2}", value);
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn settings_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{}/.config/omenspace/settings.json", home))
}

fn load_saved_scale() -> f64 {
    let Some(path) = settings_path() else { return 1.0 };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|json| json.get("ui_scale").and_then(|v| v.as_f64()))
        .map(clamp)
        .unwrap_or(1.0)
}

fn save_scale(factor: f64) {
    let Some(path) = settings_path() else { return };
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut json = serde_json::json!({});
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
            json = parsed;
        }
    }
    json["ui_scale"] = serde_json::json!(factor);
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_only_px_lengths() {
        let css = ".a { font-size: 10px; transition: all 200ms ease; opacity: 0.5; }";
        assert_eq!(
            scale_px_lengths(css, 1.5),
            ".a { font-size: 15px; transition: all 200ms ease; opacity: 0.5; }"
        );
    }

    #[test]
    fn keeps_non_ascii_and_fractions() {
        let css = "/* ── box ── */ .b { letter-spacing: -0.3px; border: 1px solid red; }";
        assert_eq!(
            scale_px_lengths(css, 2.0),
            "/* ── box ── */ .b { letter-spacing: -0.6px; border: 2px solid red; }"
        );
    }

    #[test]
    fn clamps_to_supported_range() {
        assert_eq!(clamp(5.0), MAX_SCALE);
        assert_eq!(clamp(0.1), MIN_SCALE);
        assert_eq!(clamp(1.2000001), 1.2);
    }
}
