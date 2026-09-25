import re

with open("src/omen-gui/src/keyboardrgb.rs", "r") as f:
    content = f.read()

# 1. Replace parse_hex with a fast parser
old_parse_hex = """fn parse_hex(hex: &str) -> (f64, f64, f64) {
    if hex.len() >= 7 && hex.starts_with('#') {
        let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(0) as f64 / 255.0;
        let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(0) as f64 / 255.0;
        let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(0) as f64 / 255.0;
        (r, g, b)
    } else {
        (0.1, 0.1, 0.12)
    }
}"""

new_parse_hex = """fn fast_hex_val(b: u8) -> f64 {
    match b {
        b'0'..=b'9' => (b - b'0') as f64,
        b'a'..=b'f' => (b - b'a' + 10) as f64,
        b'A'..=b'F' => (b - b'A' + 10) as f64,
        _ => 0.0,
    }
}

fn parse_hex(hex: &str) -> (f64, f64, f64) {
    let bytes = hex.as_bytes();
    if bytes.len() >= 7 && bytes[0] == b'#' {
        let r = (fast_hex_val(bytes[1]) * 16.0 + fast_hex_val(bytes[2])) / 255.0;
        let g = (fast_hex_val(bytes[3]) * 16.0 + fast_hex_val(bytes[4])) / 255.0;
        let b = (fast_hex_val(bytes[5]) * 16.0 + fast_hex_val(bytes[6])) / 255.0;
        (r, g, b)
    } else {
        (0.1, 0.1, 0.12)
    }
}"""

content = content.replace(old_parse_hex, new_parse_hex)

# 2. Modify bottom box layout
old_bottom_layout = """    speed_box.append(&speed_scale);
    bottom_box.append(&speed_box);
    bottom_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    
    let bright_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    bright_box.append(&gtk::Label::builder().label(crate::i18n::t("kb_brightness")).build());
    let bright_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    let mut bval = 100.0;
    if let Some(state_json) = state_json_opt {
        if let Some(b) = state_json["brightness"].as_f64() { bval = b; }
    }
    bright_scale.set_value(bval);
    bright_scale.set_size_request(150, -1);
    bright_scale.set_valign(gtk::Align::Center);
    bright_scale.connect_value_changed(move |sc| {
        let val = sc.value() as i32;
        crate::daemon_client::set_global_sync(val > 0, val, "ltr");
    });
    bright_box.append(&bright_scale);
    bottom_box.append(&bright_box);"""

new_bottom_layout = """    speed_box.append(&speed_scale);
    
    let bright_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    bright_box.append(&gtk::Label::builder().label(crate::i18n::t("kb_brightness")).build());
    let bright_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    let mut bval = 100.0;
    if let Some(state_json) = state_json_opt {
        if let Some(b) = state_json["brightness"].as_f64() { bval = b; }
    }
    bright_scale.set_value(bval);
    bright_scale.set_size_request(150, -1);
    bright_scale.set_valign(gtk::Align::Center);
    bright_scale.connect_value_changed(move |sc| {
        let val = sc.value() as i32;
        crate::daemon_client::set_global_sync(val > 0, val, "ltr");
    });
    bright_box.append(&bright_scale);
    
    let sliders_container = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).build();
    sliders_container.append(&speed_box);
    sliders_container.append(&bright_box);
    
    bottom_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    bottom_box.append(&sliders_container);"""

content = content.replace(old_bottom_layout, new_bottom_layout)

with open("src/omen-gui/src/keyboardrgb.rs", "w") as f:
    f.write(content)
