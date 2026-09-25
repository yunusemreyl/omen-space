use gtk::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyboardMode {
    Victus1Zone,
    Omen4Zone,
    #[allow(dead_code)]
    PerKey,
    DesktopRgb,
}


pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let mut h_prime = h / 60.0;
    while h_prime < 0.0 { h_prime += 6.0; }
    h_prime = h_prime % 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    
    let (r1, g1, b1) = if h_prime >= 0.0 && h_prime < 1.0 { (c, x, 0.0) }
    else if h_prime >= 1.0 && h_prime < 2.0 { (x, c, 0.0) }
    else if h_prime >= 2.0 && h_prime < 3.0 { (0.0, c, x) }
    else if h_prime >= 3.0 && h_prime < 4.0 { (0.0, x, c) }
    else if h_prime >= 4.0 && h_prime < 5.0 { (x, 0.0, c) }
    else { (c, 0.0, x) };
    
    (r1 + m, g1 + m, b1 + m)
}

pub fn get_zone_for_key(name: &str) -> i32 {
    if name == "zone_0" || name == "c1_btn" { return 1; }
    if name == "global_color_btn" { return 8; }
    if name == "zone_1" || name == "c2_btn" { return 2; }
    if name == "zone_2" || name == "c3_btn" { return 3; }
    if name == "zone_3" || name == "c4_btn" { return 4; }
    match name {
        "W" | "A" | "S" | "D" => 4,
        "Esc" | "F1" | "F2" | "F3" | "F4" | "~" | "1" | "2" | "3" | "4" | 
        "Tab" | "Q" | "E" | "R" | "Caps" | "F" | "Shift" | "Z" | "X" | "C" | "V" | 
        "Ctrl" | "Win" => 1,
        "F5" | "F6" | "F7" | "F8" | "5" | "6" | "7" | "8" | "T" | "Y" | "U" | "I" |
        "G" | "H" | "J" | "K" | "B" | "N" | "M" | "," | "Alt" | "Space" => 2,
        _ => 3,
    }
}

// ── Color Popover Helper ───────────────────────────────────────
#[allow(deprecated)]
pub fn show_color_picker_popover(
    parent: &gtk::Button,
    on_color_selected: Rc<dyn Fn(String)>
) {
    let popover = gtk::Popover::builder().position(gtk::PositionType::Bottom).autohide(true).build();
    popover.set_parent(parent);
    
    let container = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).margin_top(6).margin_bottom(6).margin_start(6).margin_end(6).build();
    
    // Palette: Red, Green, Blue, Purple, Turquoise, Pink, White
    let palette = ["#FF0000", "#00FF00", "#0000FF", "#800080", "#40E0D0", "#FFC0CB", "#FFFFFF"];
    let palette_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(4).build();
    for hex in palette {
        let btn = gtk::Button::builder().width_request(24).height_request(24).build();
        btn.add_css_class("circular");
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!("button {{ background: {}; min-width: 24px; min-height: 24px; padding: 0; }}", hex));
        btn.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        
        let pop_clone = popover.clone();
        let hex_clone = hex.to_string();
        let cb = on_color_selected.clone();
        btn.connect_clicked(move |_| {
            cb(hex_clone.clone());
            pop_clone.popdown();
        });
        palette_box.append(&btn);
    }
    
    let custom_btn = gtk::Button::builder().icon_name("applications-graphics-symbolic").width_request(24).height_request(24).build();
    custom_btn.add_css_class("circular");
    let cb2 = on_color_selected.clone();
    
    let parent_win_ref = parent.root().and_downcast::<gtk::Window>();
    let pop_clone2 = popover.clone();
    
    custom_btn.connect_clicked(move |_| {
        pop_clone2.popdown(); // Hide popover first
        let dialog = gtk::ColorDialog::builder().build();
        let cb_inner = cb2.clone();
        dialog.choose_rgba(parent_win_ref.as_ref(), None::<&gtk::gdk::RGBA>, None::<&gtk::gio::Cancellable>, move |res: Result<gtk::gdk::RGBA, glib::Error>| {
            if let Ok(rgba) = res {
                let hex = format!("#{:02X}{:02X}{:02X}", (rgba.red()*255.) as u8, (rgba.green()*255.) as u8, (rgba.blue()*255.) as u8);
                cb_inner(hex);
            }
        });
    });
    
    container.append(&palette_box);
    container.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    container.append(&custom_btn);
    
    popover.set_child(Some(&container));
    popover.popup();
}

// ── Interactive Keyboard Builder ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SelectionMode {
    Off,
    Key,
    #[allow(dead_code)]
    Row,
    Zone,
    All,
}

#[derive(Clone, Debug)]
pub struct KeyGeom {
    pub id: usize,
    pub name: String,
    pub display: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn fast_hex_val(b: u8) -> f64 {
    match b {
        b'0'..=b'9' => (b - b'0') as f64,
        b'a'..=b'f' => (b - b'a' + 10) as f64,
        b'A'..=b'F' => (b - b'A' + 10) as f64,
        _ => 0.0,
    }
}

pub fn parse_hex(hex: &str) -> (f64, f64, f64) {
    let bytes = hex.as_bytes();
    if bytes.len() >= 7 && bytes[0] == b'#' {
        let r = (fast_hex_val(bytes[1]) * 16.0 + fast_hex_val(bytes[2])) / 255.0;
        let g = (fast_hex_val(bytes[3]) * 16.0 + fast_hex_val(bytes[4])) / 255.0;
        let b = (fast_hex_val(bytes[5]) * 16.0 + fast_hex_val(bytes[6])) / 255.0;
        (r, g, b)
    } else {
        (0.1, 0.1, 0.12)
    }
}

pub fn draw_rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}
