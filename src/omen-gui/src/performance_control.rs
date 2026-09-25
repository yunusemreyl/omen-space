use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::i18n;
use crate::daemon_client;

pub fn build_page() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();

    // ── Section header ────────────────────────────────────────────────────────
    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(2)
        .build();
    header_row.append(&gtk::Label::builder()
        .label(i18n::t("title_performance"))
        .css_classes(["page-title"])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build());

    let daemon_pill = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(5)
        .css_classes(["daemon-pill"])
        .valign(gtk::Align::Center)
        .build();
    let daemon_dot = gtk::Label::builder().label("●").build();
    let daemon_text = gtk::Label::builder()
        .label(i18n::t("daemon_label"))
        .css_classes(["daemon-pill-text"])
        .build();
    daemon_pill.append(&daemon_dot);
    daemon_pill.append(&daemon_text);
    header_row.append(&daemon_pill);
    page.append(&header_row);

    // Subscribe to daemon status — update pill reactively
    {
        let dot = daemon_dot.clone();
        crate::daemon_client::subscribe_daemon_status(move |status| {
            use crate::daemon_client::DaemonStatus;
            if status == DaemonStatus::Online {
                dot.remove_css_class("daemon-dot-off");
                dot.add_css_class("daemon-dot-on");
            } else {
                dot.remove_css_class("daemon-dot-on");
                dot.add_css_class("daemon-dot-off");
            }
        });
        // Set initial state based on ping
        let dot_init = daemon_dot.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            if crate::daemon_client::ping_daemon_sync() {
                dot_init.remove_css_class("daemon-dot-off");
                dot_init.add_css_class("daemon-dot-on");
            } else {
                dot_init.remove_css_class("daemon-dot-on");
                dot_init.add_css_class("daemon-dot-off");
            }
        });
    }

    page.append(&gtk::Label::builder()
        .label(i18n::t("system_profiles_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .margin_bottom(8)
        .wrap(true)
        .build());

    // ── PERFORMANCE MODES ─────────────────────────────────────────────────────
    page.append(&gtk::Label::builder()
        .label(i18n::t("perf_modes_cat"))
        .css_classes(["os-cat-label"])
        .halign(gtk::Align::Start)
        .margin_bottom(6)
        .build());

    let perf_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .margin_bottom(12)
        .build();

    let (eco_btn, eco_wrap)   = build_chip_card(&crate::asset_resolver::get_asset_path("eco.svg"),         i18n::t("mode_eco"),         i18n::t("mode_eco_sub"));
    let (bal_btn, bal_wrap)   = build_chip_card(&crate::asset_resolver::get_asset_path("balanced.svg"),    i18n::t("mode_balanced"),    i18n::t("mode_balanced_sub"));
    let (perf_btn, perf_wrap) = build_chip_card(&crate::asset_resolver::get_asset_path("performance.svg"), i18n::t("mode_performance"), i18n::t("mode_performance_sub"));

    let current_power = crate::daemon_client::get_power_profile_sync();
    if current_power == "performance" {
        perf_btn.set_active(true);
    } else if current_power == "power-saver" {
        eco_btn.set_active(true);
    } else {
        bal_btn.set_active(true);
    }
    bal_btn.set_group(Some(&eco_btn));
    perf_btn.set_group(Some(&eco_btn));

    let updating_ext = Rc::new(std::cell::Cell::new(false));

    let u1 = updating_ext.clone();
    eco_btn.connect_toggled(move |btn| { if btn.is_active() && !u1.get() { daemon_client::set_power_profile_sync("power-saver".to_string()); } });
    let u2 = updating_ext.clone();
    bal_btn.connect_toggled(move |btn| { if btn.is_active() && !u2.get() { daemon_client::set_power_profile_sync("balanced".to_string()); } });
    let u3 = updating_ext.clone();
    perf_btn.connect_toggled(move |btn| { if btn.is_active() && !u3.get() { daemon_client::set_power_profile_sync("performance".to_string()); } });

    perf_box.append(&eco_wrap);
    perf_box.append(&bal_wrap);
    perf_box.append(&perf_wrap);
    page.append(&perf_box);

    // ── FAN MODES ─────────────────────────────────────────────────────────────
    let fan_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(6)
        .build();
    fan_header.append(&gtk::Label::builder()
        .label(i18n::t("fan_modes_cat"))
        .css_classes(["os-cat-label"])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build());

    let ec_btn = gtk::ToggleButton::builder().css_classes(["ec-btn"]).build();
    let ec_inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(5)
        .valign(gtk::Align::Center)
        .build();
    ec_inner.append(&gtk::Image::builder()
        .icon_name("preferences-system-symbolic")
        .pixel_size(12)
        .build());
    ec_inner.append(&gtk::Label::builder()
        .label(i18n::t("ec_delegate"))
        .css_classes(["ec-btn-lbl"])
        .build());
    ec_btn.set_child(Some(&ec_inner));
    ec_btn.set_tooltip_text(Some(i18n::t("ec_delegate_tooltip")));
    let u7 = updating_ext.clone();
    ec_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u7.get() {
            crate::daemon_client::set_fan_mode_sync("ec".to_string());
        }
    });
    fan_header.append(&ec_btn);
    page.append(&fan_header);

    let fan_box = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(4)
        .row_spacing(12)
        .column_spacing(12)
        .build();

    let (auto_btn, auto_wrap)     = build_fan_chip_card(&crate::asset_resolver::get_asset_path("balanced.svg"),    i18n::t("fan_auto"),   i18n::t("fan_auto_sub"));
    let (max_btn, max_wrap)       = build_fan_chip_card(&crate::asset_resolver::get_asset_path("performance.svg"), i18n::t("fan_max"),    i18n::t("fan_max_sub"));
    let (custom_btn, custom_wrap) = build_fan_chip_card(&crate::asset_resolver::get_asset_path("custom.svg"),      i18n::t("fan_custom"), i18n::t("fan_custom_sub"));

    let current_fan = crate::daemon_client::get_fan_mode_sync();
    if current_fan == "max" {
        max_btn.set_active(true);
    } else if current_fan == "custom" || current_fan == "manual" {
        custom_btn.set_active(true);
    } else if current_fan == "ec" {
        ec_btn.set_active(true);
    } else {
        auto_btn.set_active(true);
    }
    max_btn.set_group(Some(&auto_btn));
    custom_btn.set_group(Some(&auto_btn));
    ec_btn.set_group(Some(&auto_btn));

    let u4 = updating_ext.clone();
    auto_btn.connect_toggled(move |btn| { if btn.is_active() && !u4.get() { daemon_client::set_fan_mode_sync("auto".to_string()); } });
    let u5 = updating_ext.clone();
    max_btn.connect_toggled(move |btn| { if btn.is_active() && !u5.get() { daemon_client::set_fan_mode_sync("max".to_string()); } });
    let u6 = updating_ext.clone();
    custom_btn.connect_toggled(move |btn| { if btn.is_active() && !u6.get() { daemon_client::set_fan_mode_sync("custom".to_string()); } });

    // Live sync timer to update GUI when profile is changed from overlay/tray/CLI
    {
        let eco_c = eco_btn.clone();
        let bal_c = bal_btn.clone();
        let perf_c = perf_btn.clone();
        let auto_c = auto_btn.clone();
        let max_c = max_btn.clone();
        let custom_c = custom_btn.clone();
        let ec_c = ec_btn.clone();
        let u_sync = updating_ext.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(1500), move || {
            if !eco_c.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            #[allow(deprecated)]
            let (tx, rx) = glib::MainContext::channel::<(String, String)>(glib::Priority::default());
            let eco_c2 = eco_c.clone();
            let bal_c2 = bal_c.clone();
            let perf_c2 = perf_c.clone();
            let auto_c2 = auto_c.clone();
            let max_c2 = max_c.clone();
            let custom_c2 = custom_c.clone();
            let ec_c2 = ec_c.clone();
            let u_s2 = u_sync.clone();

            rx.attach(None, move |(p, f)| {
                u_s2.set(true);
                let p_lower = p.to_lowercase();
                if p_lower == "performance" {
                    if !perf_c2.is_active() { perf_c2.set_active(true); }
                } else if p_lower == "power-saver" || p_lower == "quiet" || p_lower == "eco" {
                    if !eco_c2.is_active() { eco_c2.set_active(true); }
                } else {
                    if !bal_c2.is_active() { bal_c2.set_active(true); }
                }

                let f_lower = f.to_lowercase();
                if f_lower == "max" {
                    if !max_c2.is_active() { max_c2.set_active(true); }
                } else if f_lower == "custom" || f_lower == "manual" {
                    if !custom_c2.is_active() { custom_c2.set_active(true); }
                } else if f_lower == "ec" {
                    if !ec_c2.is_active() { ec_c2.set_active(true); }
                } else {
                    if !auto_c2.is_active() { auto_c2.set_active(true); }
                }
                u_s2.set(false);
                glib::ControlFlow::Break
            });

            let rt = crate::daemon_client::get_runtime();
            rt.spawn(async move {
                let p = crate::daemon_client::get_power_profile_async().await.unwrap_or_else(|_| "balanced".into());
                let f = crate::daemon_client::get_fan_mode_async().await.unwrap_or_else(|_| "auto".into());
                let _ = tx.send((p, f));
            });

            glib::ControlFlow::Continue
        });
    }

    fan_box.insert(&auto_wrap, -1);
    fan_box.insert(&max_wrap, -1);
    fan_box.insert(&custom_wrap, -1);

    // Load custom presets
    let presets = crate::fan_presets::load_presets();
    for preset in presets {
        add_preset_to_box(preset, &auto_btn, &fan_box, &page);
    }

    page.append(&fan_box);

    // ── Custom curve revealer (single graph + CPU/GPU pill) ───────────────────
    let curve_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build();

    let curve_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(12)
        .css_classes(["os-card"])
        .build();

    // Header row
    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("custom_curve_title"))
        .css_classes(["os-section-header"])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build());

    // CPU / GPU pill selector
    let pill = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["curve-pill"])
        .valign(gtk::Align::Center)
        .build();
    let cpu_pill_btn = gtk::ToggleButton::builder()
        .label(i18n::t("cpu"))
        .css_classes(["curve-pill-btn"])
        .active(true)
        .build();
    let gpu_pill_btn = gtk::ToggleButton::builder()
        .label(i18n::t("gpu"))
        .css_classes(["curve-pill-btn"])
        .group(&cpu_pill_btn)
        .build();
    pill.append(&cpu_pill_btn);
    pill.append(&gpu_pill_btn);
    hdr.append(&pill);
    curve_card.append(&hdr);

    // Hint text
    curve_card.append(&gtk::Label::builder()
        .label(i18n::t("custom_curve_hint"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());

    // Control points for CPU and GPU (temp 40..100, speed 0..100)
    let cpu_pts: Rc<RefCell<Vec<(f64, f64)>>> = Rc::new(RefCell::new(vec![
        (40.0, 20.0), (55.0, 35.0), (70.0, 60.0), (85.0, 82.0), (100.0, 100.0),
    ]));
    let gpu_pts: Rc<RefCell<Vec<(f64, f64)>>> = Rc::new(RefCell::new(vec![
        (40.0, 15.0), (55.0, 30.0), (70.0, 55.0), (85.0, 78.0), (100.0, 100.0),
    ]));

    // Which curve is active: false = CPU, true = GPU
    let show_gpu: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // Square drawing area
    let da = gtk::DrawingArea::builder()
        .width_request(320)
        .height_request(320)
        .hexpand(false)
        .vexpand(false)
        .halign(gtk::Align::Center)
        .build();

    // ── Draw function ─────────────────────────────────────────────────────────
    let cpu_d = cpu_pts.clone();
    let gpu_d = gpu_pts.clone();
    let show_d = show_gpu.clone();
    da.set_draw_func(move |_, cr, w, h| {
        let is_gpu = *show_d.borrow();
        let pts = if is_gpu { gpu_d.borrow() } else { cpu_d.borrow() };
        let (r, g, b) = if is_gpu { (0.2, 0.8, 1.0) } else { (1.0, 0.25, 0.4) };

        let pad = 36.0_f64;
        let aw  = w as f64 - 2.0 * pad;
        let ah  = h as f64 - 2.0 * pad;

        let to_canvas = |temp: f64, speed: f64| -> (f64, f64) {
            (pad + (temp - 40.0) / 60.0 * aw,
             pad + (1.0 - speed / 100.0) * ah)
        };

        // Background transparent
        cr.set_operator(gtk::cairo::Operator::Clear);
        cr.paint().expect("Invalid cairo surface");
        cr.set_operator(gtk::cairo::Operator::Over);

        // Subtle grid
        cr.set_line_width(0.5);
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.15);
        for i in 1..=4 {
            let x = pad + aw * i as f64 / 4.0;
            cr.move_to(x, pad); cr.line_to(x, pad + ah); let _ = cr.stroke();
            let y = pad + ah * i as f64 / 4.0;
            cr.move_to(pad, y); cr.line_to(pad + aw, y); let _ = cr.stroke();
        }

        // Axes border
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.3);
        cr.set_line_width(1.0);
        cr.rectangle(pad, pad, aw, ah);
        let _ = cr.stroke();

        // Axis labels
        cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
        cr.set_font_size(9.0);
        cr.set_source_rgba(0.5, 0.5, 0.5, 1.0);
        for (i, temp) in [40, 55, 70, 85, 100].iter().enumerate() {
            let x = pad + aw * i as f64 / 4.0;
            cr.move_to(x - 8.0, pad + ah + 14.0);
            let _ = cr.show_text(&format!("{}°C", temp));
        }
        for (i, pct) in ["100%", " 75%", " 50%", " 25%", "  0%"].iter().enumerate() {
            let y = pad + ah * i as f64 / 4.0;
            cr.move_to(2.0, y + 4.0);
            let _ = cr.show_text(pct);
        }

        // Y axis label
        cr.set_font_size(8.5);
        cr.set_source_rgba(0.5, 0.5, 0.5, 1.0);
        cr.save().unwrap();
        cr.translate(10.0, pad + ah / 2.0);
        cr.rotate(-std::f64::consts::FRAC_PI_2);
        cr.move_to(-22.0, 0.0);
        let _ = cr.show_text("FAN %");
        cr.restore().unwrap();

        // X axis label
        cr.move_to(pad + aw / 2.0 - 22.0, pad + ah + 26.0);
        let _ = cr.show_text("TEMP °C");

        // Fill under curve
        if pts.is_empty() { return; }
        
        let (x0, y0) = to_canvas(pts[0].0, pts[0].1);
        cr.move_to(x0, pad + ah);
        cr.line_to(x0, y0);
        for p in pts.iter().skip(1) {
            let (x, y) = to_canvas(p.0, p.1);
            cr.line_to(x, y);
        }
        if let Some(last) = pts.last() {
            let (xl, _) = to_canvas(last.0, last.1);
            cr.line_to(xl, pad + ah);
        }
        cr.close_path();
        cr.set_source_rgba(r, g, b, 0.10);
        let _ = cr.fill();

        // Curve line
        cr.set_source_rgba(r, g, b, 1.0);
        cr.set_line_width(2.5);
        let (x0, y0) = to_canvas(pts[0].0, pts[0].1);
        cr.move_to(x0, y0);
        for p in pts.iter().skip(1) {
            let (x, y) = to_canvas(p.0, p.1);
            cr.line_to(x, y);
        }
        let _ = cr.stroke();

        // Control point circles + value labels
        for p in pts.iter() {
            let (x, y) = to_canvas(p.0, p.1);
            cr.set_source_rgb(r, g, b);
            cr.arc(x, y, 5.5, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.75);
            cr.set_line_width(1.0);
            cr.arc(x, y, 5.5, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            // value label
            cr.set_font_size(8.5);
            cr.set_source_rgba(r, g, b, 0.85);
            cr.move_to(x + 8.0, y - 5.0);
            let _ = cr.show_text(&format!("{:.0}%", p.1));
        }
    });

    // ── Gesture drag ─────────────────────────────────────────────────────────
    let dragging: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

    let gesture = gtk::GestureDrag::new();

    let cpu_p  = cpu_pts.clone();
    let gpu_p  = gpu_pts.clone();
    let show_p = show_gpu.clone();
    let drag_p = dragging.clone();
    let da_p   = da.clone();
    gesture.connect_drag_begin(move |_, x, y| {
        let is_gpu = *show_p.borrow();
        let pts = if is_gpu { gpu_p.borrow() } else { cpu_p.borrow() };
        let pad = 36.0_f64;
        let aw  = da_p.width()  as f64 - 2.0 * pad;
        let ah  = da_p.height() as f64 - 2.0 * pad;
        for (i, p) in pts.iter().enumerate() {
            let cx = pad + (p.0 - 40.0) / 60.0 * aw;
            let cy = pad + (1.0 - p.1 / 100.0) * ah;
            if ((cx - x).powi(2) + (cy - y).powi(2)).sqrt() < 14.0 {
                *drag_p.borrow_mut() = Some(i);
                return;
            }
        }
    });

    let cpu_u  = cpu_pts.clone();
    let gpu_u  = gpu_pts.clone();
    let show_u = show_gpu.clone();
    let drag_u = dragging.clone();
    let da_u   = da.clone();
    gesture.connect_drag_update(move |g, _ox, oy| {
        let idx = match *drag_u.borrow() { Some(i) => i, None => return };
        if let Some((_sx, sy)) = g.start_point() {
            let pad = 36.0_f64;
            let ah  = da_u.height() as f64 - 2.0 * pad;
            let y   = (sy + oy).clamp(pad, pad + ah);
            let new_speed = ((1.0 - (y - pad) / ah) * 100.0).clamp(0.0, 100.0);
            let is_gpu = *show_u.borrow();
            if is_gpu { gpu_u.borrow_mut()[idx].1 = new_speed; }
            else       { cpu_u.borrow_mut()[idx].1 = new_speed; }
            da_u.queue_draw();
        }
    });

    let drag_e = dragging.clone();
    gesture.connect_drag_end(move |_, _, _| { *drag_e.borrow_mut() = None; });
    da.add_controller(gesture);

    // ── CPU / GPU pill toggle ─────────────────────────────────────────────────
    let show_cpu_pill = show_gpu.clone();
    let da_cpu_pill   = da.clone();
    cpu_pill_btn.connect_toggled(move |btn| {
        if btn.is_active() {
            *show_cpu_pill.borrow_mut() = false;
            da_cpu_pill.queue_draw();
        }
    });

    let show_gpu_pill = show_gpu.clone();
    let da_gpu_pill   = da.clone();
    gpu_pill_btn.connect_toggled(move |btn| {
        if btn.is_active() {
            *show_gpu_pill.borrow_mut() = true;
            da_gpu_pill.queue_draw();
        }
    });

    curve_card.append(&da);

    let bottom_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .build();

    let preset_name_entry = gtk::Entry::builder()
        .placeholder_text(i18n::t("preset_name"))
        .hexpand(true)
        .build();
    bottom_box.append(&preset_name_entry);

    let save_preset_btn = gtk::Button::builder()
        .label(i18n::t("save_preset"))
        .build();
    bottom_box.append(&save_preset_btn);

    let delete_btn = gtk::Button::builder()
        .label(i18n::t("delete_preset"))
        .css_classes(["destructive-action"])
        .build();
    bottom_box.append(&delete_btn);

    let apply_btn = gtk::Button::builder()
        .label(i18n::t("apply"))
        .css_classes(["suggested-action"])
        .build();
    bottom_box.append(&apply_btn);
    curve_card.append(&bottom_box);
    curve_revealer.set_child(Some(&curve_card));

    let rev1 = curve_revealer.clone();
    custom_btn.connect_toggled(move |b| { rev1.set_reveal_child(b.is_active()); });

    let click_gesture = gtk::GestureClick::new();
    let rev_click = curve_revealer.clone();
    let btn_clone = custom_btn.clone();
    click_gesture.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 && btn_clone.is_active() {
            rev_click.set_reveal_child(true);
        }
    });
    custom_btn.add_controller(click_gesture);
    let rev2 = curve_revealer.clone();
    let cpu_pts_c = cpu_pts.clone();
    apply_btn.connect_clicked(move |_| {
        let pts = cpu_pts_c.borrow().clone();
        if let Ok(json) = serde_json::to_string(&pts) {
            crate::daemon_client::save_custom_curve_sync(json);
        }
        rev2.set_reveal_child(false);
    });

    let cpu_pts_save = cpu_pts.clone();
    let fan_box_clone = fan_box.clone();
    let auto_btn_clone = auto_btn.clone();
    let preset_name_save = preset_name_entry.clone();
    let page_save = page.clone();
    save_preset_btn.connect_clicked(move |_| {
        let name = preset_name_save.text().to_string();
        if name.trim().is_empty() { return; }
        let pts = cpu_pts_save.borrow().clone();
        let preset = crate::fan_presets::FanPreset { name: name.clone(), points: pts.clone() };
        let mut loaded = crate::fan_presets::load_presets();
        
        let mut exists = false;
        if let Some(pos) = loaded.iter().position(|p| p.name == name) {
            loaded[pos] = preset.clone();
            exists = true;
        } else {
            loaded.push(preset.clone());
        }
        crate::fan_presets::save_presets(&loaded);

        if !exists {
            // Dynamically add to fan box using helper
            add_preset_to_box(preset.clone(), &auto_btn_clone, &fan_box_clone, &page_save);
        }
        preset_name_save.set_text("");
    });

    let fan_box_del = fan_box.clone();
    let entry_del = preset_name_entry.clone();
    delete_btn.connect_clicked(move |_| {
        let name = entry_del.text().to_string();
        if name.trim().is_empty() { return; }
        crate::fan_presets::delete_preset(&name);
        
        // Remove from UI dynamically
        let mut child = fan_box_del.first_child();
        while let Some(c) = child {
            if let Some(inner) = c.first_child() {
                if inner.widget_name() == name {
                    fan_box_del.remove(&c);
                    break;
                }
            }
            child = c.next_sibling();
        }
        entry_del.set_text("");
    });

    page.append(&curve_revealer);
    page
}

/// Thick rectangular chip toggle card (for performance modes).
pub fn build_chip_card(icon_path: &str, title: &str, subtitle: &str) -> (gtk::ToggleButton, gtk::Box) {
    let btn = gtk::ToggleButton::builder()
        .css_classes(["chip-card"])
        .build();

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();

    inner.append(&gtk::Image::builder().file(icon_path).pixel_size(28).build());

    let text_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .valign(gtk::Align::Center)
        .build();
    text_col.append(&gtk::Label::builder()
        .label(title).css_classes(["chip-title"]).halign(gtk::Align::Start).build());
    text_col.append(&gtk::Label::builder()
        .label(subtitle).css_classes(["chip-sub"]).halign(gtk::Align::Start).build());
    inner.append(&text_col);
    btn.set_child(Some(&inner));

    let wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    wrap.append(&btn);
    (btn, wrap)
}

/// Thin compact rectangular chip toggle card (for fan modes).
pub fn build_fan_chip_card(icon_path: &str, title: &str, subtitle: &str) -> (gtk::ToggleButton, gtk::Box) {
    let btn = gtk::ToggleButton::builder()
        .css_classes(["fan-chip-card"])
        .build();

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();

    inner.append(&gtk::Image::builder().file(icon_path).pixel_size(20).build());

    let text_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(1)
        .valign(gtk::Align::Center)
        .build();
    text_col.append(&gtk::Label::builder()
        .label(title).css_classes(["fan-chip-title"]).halign(gtk::Align::Start).build());
    text_col.append(&gtk::Label::builder()
        .label(subtitle).css_classes(["fan-chip-sub"]).halign(gtk::Align::Start).build());
    inner.append(&text_col);
    btn.set_child(Some(&inner));

    let wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    wrap.append(&btn);
    (btn, wrap)
}


fn add_preset_to_box(
    preset: crate::fan_presets::FanPreset,
    auto_btn: &gtk::ToggleButton,
    fan_box: &gtk::FlowBox,
    page: &gtk::Box,
) {
    let preset_clone = preset.clone();
    let (p_btn, p_wrap) = build_fan_chip_card(&crate::asset_resolver::get_asset_path("custom.svg"), &preset.name, i18n::t("preset_sub"));
    p_wrap.set_widget_name(&preset.name);
    p_btn.set_group(Some(auto_btn));
    p_btn.connect_toggled(move |btn| {
        if btn.is_active() {
            if let Ok(json) = serde_json::to_string(&preset_clone.points) {
                crate::daemon_client::save_custom_curve_sync(json);
            }
            crate::daemon_client::set_fan_mode_sync("custom".to_string());
        }
    });
    
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&p_wrap));
    
    let actions_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(4)
        .margin_end(4)
        .build();
        
    let edit_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .css_classes(["circular", "flat"])
        .build();
        
    let del_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .css_classes(["circular", "flat", "destructive-action"])
        .build();
        
    actions_box.append(&edit_btn);
    actions_box.append(&del_btn);
    actions_box.set_opacity(0.0);
    
    overlay.add_overlay(&actions_box);
    
    let hover = gtk::EventControllerMotion::new();
    let act_clone = actions_box.clone();
    hover.connect_enter(move |_, _, _| { act_clone.set_opacity(1.0); });
    let act_clone2 = actions_box.clone();
    hover.connect_leave(move |_| { act_clone2.set_opacity(0.0); });
    overlay.add_controller(hover);
    
    let win = page.clone();
    let p_name = preset.name.clone();
    let p_points = preset.points.clone();
    let fan_box_c = fan_box.clone();
    let overlay_c = overlay.clone();
    edit_btn.connect_clicked(move |_| {
        let p_n = p_name.clone();
        let p_n1 = p_n.clone();
        let p_n2 = p_n.clone();
        let f_box = fan_box_c.clone();
        let o_lay = overlay_c.clone();
        if let Some(w) = win.root().and_downcast::<gtk::ApplicationWindow>() {
            crate::fan_curve_editor::show_fan_curve_editor(
                &w,
                &p_n,
                p_points.clone(),
                move |new_pts| {
                    let mut p = crate::fan_presets::load_presets();
                    if let Some(x) = p.iter_mut().find(|x| x.name == p_n1) {
                        x.points = new_pts;
                        crate::fan_presets::save_presets(&p);
                    }
                },
                move || {
                    let p = crate::fan_presets::load_presets();
                    let p: Vec<_> = p.into_iter().filter(|x| x.name != p_n2).collect();
                    crate::fan_presets::save_presets(&p);
                    f_box.remove(&o_lay);
                }
            );
        }
    });
    
    let p_name2 = preset.name.clone();
    let fan_box_c2 = fan_box.clone();
    let overlay_c2 = overlay.clone();
    del_btn.connect_clicked(move |_| {
        let p = crate::fan_presets::load_presets();
        let p: Vec<_> = p.into_iter().filter(|x| x.name != p_name2).collect();
        crate::fan_presets::save_presets(&p);
        fan_box_c2.remove(&overlay_c2);
    });

    fan_box.append(&overlay);
}
