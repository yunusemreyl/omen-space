use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::daemon_client;
use crate::i18n;

/// Builds a concise, human-readable one-line hardware identity string for the Performance header.
fn format_hardware_subtitle(specs: &omen_types::HardwareSpecs) -> String {
    let mut parts: Vec<String> = Vec::new();
    let product = specs.product_name.trim();
    if !product.is_empty() && product != "HP Device" {
        parts.push(product.to_string());
    }
    let cpu = specs.cpu_spec.split('·').next().unwrap_or("").trim();
    if !cpu.is_empty() && cpu != "Unknown CPU" {
        parts.push(cpu.to_string());
    }
    let gpu = specs.gpu_spec.split('·').next().unwrap_or("").trim();
    if !gpu.is_empty() && gpu != "Unknown GPU" {
        parts.push(gpu.to_string());
    }
    if parts.is_empty() {
        i18n::t("system_profiles_desc").to_string()
    } else {
        parts.join("  ·  ")
    }
}

pub fn build_page() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Start)
        .spacing(0)
        .build();

    let specs = daemon_client::get_hardware_specs_sync();

    // ── Header Row ────────────────────────────────────────────────────────────
    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_bottom(22)
        .build();

    let title_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();

    title_col.append(
        &gtk::Label::builder()
            .label(i18n::t("title_performance"))
            .css_classes(["page-title"])
            .halign(gtk::Align::Start)
            .build(),
    );

    let hw_subtitle = format_hardware_subtitle(&specs);
    title_col.append(
        &gtk::Label::builder()
            .label(&hw_subtitle)
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .tooltip_text(&hw_subtitle)
            .build(),
    );
    header_row.append(&title_col);

    // Daemon live status pill
    let daemon_pill = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
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

    {
        let dot = daemon_dot.clone();
        daemon_client::subscribe_daemon_status(move |status| {
            use crate::daemon_client::DaemonStatus;
            if status == DaemonStatus::Online {
                dot.remove_css_class("daemon-dot-off");
                dot.add_css_class("daemon-dot-on");
            } else {
                dot.remove_css_class("daemon-dot-on");
                dot.add_css_class("daemon-dot-off");
            }
        });
    }

    // ── Section 1: Performance Modes ──────────────────────────────────────────
    page.append(
        &gtk::Label::builder()
            .label(i18n::t("perf_modes_heading"))
            .css_classes(["os-section-header"])
            .halign(gtk::Align::Start)
            .margin_bottom(10)
            .build(),
    );

    let perf_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .valign(gtk::Align::Start)
        .margin_bottom(26)
        .build();

    let (eco_btn, eco_wrap) = build_mode_card(
        &crate::asset_resolver::get_asset_path("eco.svg"),
        i18n::t("mode_eco"),
        i18n::t("mode_eco_sub"),
        i18n::t("mode_eco_trait"),
    );
    let (bal_btn, bal_wrap) = build_mode_card(
        &crate::asset_resolver::get_asset_path("balanced.svg"),
        i18n::t("mode_balanced"),
        i18n::t("mode_balanced_sub"),
        i18n::t("mode_balanced_trait"),
    );
    let (perf_btn, perf_wrap) = build_mode_card(
        &crate::asset_resolver::get_asset_path("performance.svg"),
        i18n::t("mode_performance"),
        i18n::t("mode_performance_sub"),
        i18n::t("mode_performance_trait"),
    );

    let current_power = daemon_client::get_power_profile_sync();
    if current_power == "performance" {
        perf_btn.set_active(true);
    } else if current_power == "power-saver" {
        eco_btn.set_active(true);
    } else {
        bal_btn.set_active(true);
    }
    bal_btn.set_group(Some(&eco_btn));
    perf_btn.set_group(Some(&eco_btn));

    let perf_error_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["os-panel"])
        .margin_bottom(18)
        .visible(false)
        .build();
    let perf_err_icon = gtk::Image::builder()
        .icon_name("dialog-warning-symbolic")
        .css_classes(["warning"])
        .valign(gtk::Align::Start)
        .margin_top(2)
        .build();
    let perf_error_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .hexpand(true)
        .css_classes(["warning"])
        .build();
    perf_error_box.append(&perf_err_icon);
    perf_error_box.append(&perf_error_label);

    let updating_ext = Rc::new(std::cell::Cell::new(false));

    let apply_power_profile_ui = {
        let eco_r = eco_btn.clone();
        let bal_r = bal_btn.clone();
        let perf_r = perf_btn.clone();
        let u_r = updating_ext.clone();
        let err_box_r = perf_error_box.clone();
        let err_lbl_r = perf_error_label.clone();
        move |target: &str| {
            let eco_c = eco_r.clone();
            let bal_c = bal_r.clone();
            let perf_c = perf_r.clone();
            let u_c = u_r.clone();
            let err_box_c = err_box_r.clone();
            let err_lbl_c = err_lbl_r.clone();
            daemon_client::set_power_profile_with_result(
                target.to_string(),
                move |res| match res {
                    Ok(()) => {
                        err_box_c.set_visible(false);
                    }
                    Err((err_msg, actual)) => {
                        u_c.set(true);
                        let p_lower = actual.to_lowercase();
                        if p_lower == "performance" {
                            perf_c.set_active(true);
                        } else if p_lower == "power-saver" || p_lower == "quiet" || p_lower == "eco"
                        {
                            eco_c.set_active(true);
                        } else {
                            bal_c.set_active(true);
                        }
                        u_c.set(false);
                        err_lbl_c.set_label(&format!(
                            "{}: {}",
                            i18n::t("perf_profile_apply_failed"),
                            err_msg
                        ));
                        err_box_c.set_visible(true);
                    }
                },
            );
        }
    };

    let u1 = updating_ext.clone();
    let apply1 = apply_power_profile_ui.clone();
    eco_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u1.get() {
            apply1("power-saver");
        }
    });
    let u2 = updating_ext.clone();
    let apply2 = apply_power_profile_ui.clone();
    bal_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u2.get() {
            apply2("balanced");
        }
    });
    let u3 = updating_ext.clone();
    let apply3 = apply_power_profile_ui;
    perf_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u3.get() {
            apply3("performance");
        }
    });

    perf_box.append(&eco_wrap);
    perf_box.append(&bal_wrap);
    perf_box.append(&perf_wrap);
    page.append(&perf_box);
    page.append(&perf_error_box);

    // ── Section 2: Fan Control ────────────────────────────────────────────────
    let fan_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(10)
        .build();
    fan_header.append(
        &gtk::Label::builder()
            .label(i18n::t("fan_modes_heading"))
            .css_classes(["os-section-header"])
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build(),
    );

    let new_preset_btn = gtk::Button::builder().css_classes(["ec-btn"]).build();
    let np_inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    np_inner.append(
        &gtk::Image::builder()
            .file(crate::asset_resolver::get_asset_path("fan_add_preset.svg"))
            .pixel_size(14)
            .build(),
    );
    np_inner.append(
        &gtk::Label::builder()
            .label(i18n::t("new_preset"))
            .css_classes(["ec-btn-lbl"])
            .build(),
    );
    new_preset_btn.set_child(Some(&np_inner));
    new_preset_btn.set_tooltip_text(Some(i18n::t("new_preset_tooltip")));
    fan_header.append(&new_preset_btn);

    let ec_btn = gtk::ToggleButton::builder().css_classes(["ec-btn"]).build();
    let ec_inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    ec_inner.append(
        &gtk::Image::builder()
            .file(crate::asset_resolver::get_asset_path("fan_ec.svg"))
            .pixel_size(14)
            .build(),
    );
    ec_inner.append(
        &gtk::Label::builder()
            .label(i18n::t("ec_delegate"))
            .css_classes(["ec-btn-lbl"])
            .build(),
    );
    ec_btn.set_child(Some(&ec_inner));
    ec_btn.set_tooltip_text(Some(i18n::t("ec_delegate_tooltip")));
    let u7 = updating_ext.clone();
    ec_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u7.get() {
            daemon_client::set_fan_mode_sync("ec".to_string());
        }
    });
    fan_header.append(&ec_btn);
    page.append(&fan_header);

    let fan_box = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(4)
        .max_children_per_line(4)
        .row_spacing(10)
        .column_spacing(10)
        .build();

    let (auto_btn, auto_wrap) = build_fan_card(
        &crate::asset_resolver::get_asset_path("fan_auto.svg"),
        i18n::t("fan_auto"),
        i18n::t("fan_auto_sub"),
    );
    let (max_btn, max_wrap) = build_fan_card(
        &crate::asset_resolver::get_asset_path("fan_max.svg"),
        i18n::t("fan_max"),
        i18n::t("fan_max_sub"),
    );
    let (custom_btn, custom_wrap) = build_fan_card(
        &crate::asset_resolver::get_asset_path("fan_custom.svg"),
        i18n::t("fan_custom"),
        i18n::t("fan_custom_sub"),
    );
    auto_btn.set_tooltip_text(Some(i18n::t("fan_auto_tooltip")));
    max_btn.set_tooltip_text(Some(i18n::t("fan_max_tooltip")));
    custom_btn.set_tooltip_text(Some(i18n::t("fan_custom_tooltip")));

    let current_fan = daemon_client::get_fan_mode_sync();
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
    auto_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u4.get() {
            daemon_client::set_fan_mode_sync("auto".to_string());
        }
    });
    let u5 = updating_ext.clone();
    max_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u5.get() {
            daemon_client::set_fan_mode_sync("max".to_string());
        }
    });
    let u6 = updating_ext.clone();
    custom_btn.connect_toggled(move |btn| {
        if btn.is_active() && !u6.get() {
            daemon_client::set_fan_mode_sync("custom".to_string());
        }
    });

    // Control points for CPU and GPU (temp 40..100, speed 0..100)
    let cpu_pts: Rc<RefCell<Vec<(f64, f64)>>> = Rc::new(RefCell::new(vec![
        (40.0, 20.0),
        (55.0, 35.0),
        (70.0, 60.0),
        (85.0, 82.0),
        (100.0, 100.0),
    ]));
    let gpu_pts: Rc<RefCell<Vec<(f64, f64)>>> = Rc::new(RefCell::new(vec![
        (40.0, 15.0),
        (55.0, 30.0),
        (70.0, 55.0),
        (85.0, 78.0),
        (100.0, 100.0),
    ]));
    let preset_btns: Rc<RefCell<Vec<gtk::ToggleButton>>> = Rc::new(RefCell::new(Vec::new()));

    // Live sync timer to reflect profile/fan changes from overlay, tray, or CLI
    {
        let eco_c = eco_btn.clone();
        let bal_c = bal_btn.clone();
        let perf_c = perf_btn.clone();
        let auto_c = auto_btn.clone();
        let max_c = max_btn.clone();
        let custom_c = custom_btn.clone();
        let ec_c = ec_btn.clone();
        let u_sync = updating_ext.clone();
        let p_btns_c = preset_btns.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(1500), move || {
            if !eco_c.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            #[allow(deprecated)]
            let (tx, rx) =
                glib::MainContext::channel::<(String, String)>(glib::Priority::default());
            let eco_c2 = eco_c.clone();
            let bal_c2 = bal_c.clone();
            let perf_c2 = perf_c.clone();
            let auto_c2 = auto_c.clone();
            let max_c2 = max_c.clone();
            let custom_c2 = custom_c.clone();
            let ec_c2 = ec_c.clone();
            let u_s2 = u_sync.clone();
            let p_btns_c2 = p_btns_c.clone();

            rx.attach(None, move |(p, f)| {
                u_s2.set(true);
                let p_lower = p.to_lowercase();
                if p_lower == "performance" {
                    if !perf_c2.is_active() {
                        perf_c2.set_active(true);
                    }
                } else if p_lower == "power-saver" || p_lower == "quiet" || p_lower == "eco" {
                    if !eco_c2.is_active() {
                        eco_c2.set_active(true);
                    }
                } else if !bal_c2.is_active() {
                    bal_c2.set_active(true);
                }

                let f_lower = f.to_lowercase();
                if f_lower == "max" {
                    if !max_c2.is_active() {
                        max_c2.set_active(true);
                    }
                } else if f_lower == "custom" || f_lower == "manual" {
                    let any_preset_active = p_btns_c2.borrow().iter().any(|b| b.is_active());
                    if !custom_c2.is_active() && !any_preset_active {
                        custom_c2.set_active(true);
                    }
                } else if f_lower == "ec" {
                    if !ec_c2.is_active() {
                        ec_c2.set_active(true);
                    }
                } else if !auto_c2.is_active() {
                    auto_c2.set_active(true);
                }
                u_s2.set(false);
                glib::ControlFlow::Break
            });

            let rt = daemon_client::get_runtime();
            rt.spawn(async move {
                let p = daemon_client::get_power_profile_async()
                    .await
                    .unwrap_or_else(|_| "balanced".into());
                let f = daemon_client::get_fan_mode_async()
                    .await
                    .unwrap_or_else(|_| "auto".into());
                let _ = tx.send((p, f));
            });

            glib::ControlFlow::Continue
        });
    }

    fan_box.insert(&auto_wrap, -1);
    fan_box.insert(&max_wrap, -1);
    fan_box.insert(&custom_wrap, -1);
    page.append(&fan_box);

    // ── Custom Fan Curve Workspace (Revealer) ─────────────────────────────────
    let curve_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(200)
        .reveal_child(current_fan == "custom" || current_fan == "manual")
        .build();

    let curve_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .css_classes(["os-panel"])
        .build();

    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    let hdr_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    hdr_text.append(
        &gtk::Label::builder()
            .label(i18n::t("custom_curve_title"))
            .css_classes(["os-section-header"])
            .halign(gtk::Align::Start)
            .build(),
    );
    hdr_text.append(
        &gtk::Label::builder()
            .label(i18n::t("custom_curve_hint"))
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .build(),
    );
    hdr.append(&hdr_text);

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

    let show_gpu: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // Full-width responsive drawing area
    let da = gtk::DrawingArea::builder()
        .height_request(236)
        .hexpand(true)
        .vexpand(false)
        .build();

    // Load custom presets into fan_box
    let presets = crate::fan_presets::load_presets();
    for preset in presets {
        add_preset_to_box(
            preset,
            &auto_btn,
            &fan_box,
            &page,
            &cpu_pts,
            &gpu_pts,
            &da,
            &preset_btns,
        );
    }

    // Hydrate saved curves from daemon
    {
        let cpu_pts_init = cpu_pts.clone();
        let gpu_pts_init = gpu_pts.clone();
        let da_init = da.clone();
        glib::spawn_future_local(async move {
            if let Ok(fan_json) = daemon_client::get_fan_info_async().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&fan_json) {
                    let mut cpu_loaded: Option<Vec<(f64, f64)>> = None;
                    if let Some(curve_str) = val.get("custom_curve").and_then(|v| v.as_str()) {
                        if let Ok(pts) = serde_json::from_str::<Vec<(f64, f64)>>(curve_str) {
                            if !pts.is_empty() {
                                *cpu_pts_init.borrow_mut() = pts.clone();
                                cpu_loaded = Some(pts);
                            }
                        }
                    }
                    let gpu_loaded = val
                        .get("custom_curve_gpu")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str::<Vec<(f64, f64)>>(s).ok())
                        .filter(|pts| !pts.is_empty());
                    if let Some(pts) = gpu_loaded.or(cpu_loaded) {
                        *gpu_pts_init.borrow_mut() = pts;
                    }
                    da_init.queue_draw();
                }
            }
        });
    }

    // Padding constants shared between draw and drag gesture
    const PAD_L: f64 = 44.0;
    const PAD_R: f64 = 20.0;
    const PAD_T: f64 = 18.0;
    const PAD_B: f64 = 28.0;

    let cpu_d = cpu_pts.clone();
    let gpu_d = gpu_pts.clone();
    let show_d = show_gpu.clone();
    da.set_draw_func(move |widget, cr, w, h| {
        let is_gpu = *show_d.borrow();
        let pts = if is_gpu {
            gpu_d.borrow()
        } else {
            cpu_d.borrow()
        };
        let (r, g, b) = if is_gpu {
            (0.17, 0.60, 0.95)
        } else {
            (0.88, 0.27, 0.38)
        };

        let fg = widget.color();
        let (fr, fg_c, fb) = (fg.red() as f64, fg.green() as f64, fg.blue() as f64);

        let aw = (w as f64 - PAD_L - PAD_R).max(1.0);
        let ah = (h as f64 - PAD_T - PAD_B).max(1.0);

        let to_canvas = |temp: f64, speed: f64| -> (f64, f64) {
            (
                PAD_L + (temp - 40.0) / 60.0 * aw,
                PAD_T + (1.0 - speed / 100.0) * ah,
            )
        };

        cr.set_operator(gtk::cairo::Operator::Clear);
        let _ = cr.paint();
        cr.set_operator(gtk::cairo::Operator::Over);

        // Horizontal and vertical grid lines
        cr.set_line_width(1.0);
        cr.set_source_rgba(fr, fg_c, fb, 0.08);
        for i in 0..=4 {
            let x = (PAD_L + aw * i as f64 / 4.0).round() + 0.5;
            cr.move_to(x, PAD_T);
            cr.line_to(x, PAD_T + ah);
            let _ = cr.stroke();

            let y = (PAD_T + ah * i as f64 / 4.0).round() + 0.5;
            cr.move_to(PAD_L, y);
            cr.line_to(PAD_L + aw, y);
            let _ = cr.stroke();
        }

        // Axis labels
        cr.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        cr.set_font_size(10.0);
        cr.set_source_rgba(fr, fg_c, fb, 0.52);

        for (i, temp) in [40, 55, 70, 85, 100].iter().enumerate() {
            let x = PAD_L + aw * i as f64 / 4.0;
            let text = format!("{}°C", temp);
            let tw = cr.text_extents(&text).map(|e| e.width()).unwrap_or(20.0);
            cr.move_to(
                (x - tw / 2.0).clamp(PAD_L - 6.0, w as f64 - tw - 2.0),
                PAD_T + ah + 17.0,
            );
            let _ = cr.show_text(&text);
        }
        for (i, pct) in ["100%", "75%", "50%", "25%", "0%"].iter().enumerate() {
            let y = PAD_T + ah * i as f64 / 4.0;
            let tw = cr.text_extents(pct).map(|e| e.width()).unwrap_or(24.0);
            cr.move_to(PAD_L - tw - 8.0, y + 3.5);
            let _ = cr.show_text(pct);
        }

        if pts.is_empty() {
            return;
        }

        // Gradient fill under curve
        let (x0, y0) = to_canvas(pts[0].0, pts[0].1);
        cr.move_to(x0, PAD_T + ah);
        cr.line_to(x0, y0);
        for p in pts.iter().skip(1) {
            let (x, y) = to_canvas(p.0, p.1);
            cr.line_to(x, y);
        }
        if let Some(last) = pts.last() {
            let (xl, _) = to_canvas(last.0, last.1);
            cr.line_to(xl, PAD_T + ah);
        }
        cr.close_path();
        let grad = gtk::cairo::LinearGradient::new(0.0, PAD_T, 0.0, PAD_T + ah);
        grad.add_color_stop_rgba(0.0, r, g, b, 0.22);
        grad.add_color_stop_rgba(1.0, r, g, b, 0.02);
        let _ = cr.set_source(&grad);
        let _ = cr.fill();

        // Curve stroke
        cr.set_source_rgba(r, g, b, 1.0);
        cr.set_line_width(2.4);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.move_to(x0, y0);
        for p in pts.iter().skip(1) {
            let (x, y) = to_canvas(p.0, p.1);
            cr.line_to(x, y);
        }
        let _ = cr.stroke();

        // Control handles + labels
        cr.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Bold,
        );
        cr.set_font_size(9.5);
        for p in pts.iter() {
            let (x, y) = to_canvas(p.0, p.1);

            // Outer soft halo
            cr.set_source_rgba(r, g, b, 0.22);
            cr.arc(x, y, 9.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();

            // Solid node
            cr.set_source_rgb(r, g, b);
            cr.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();

            // Inner ring
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
            cr.set_line_width(1.5);
            cr.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();

            // Value label
            let lbl = format!("{:.0}%", p.1);
            let tw = cr.text_extents(&lbl).map(|e| e.width()).unwrap_or(20.0);
            let lx = (x - tw / 2.0).clamp(PAD_L, PAD_L + aw - tw);
            let ly = if y < PAD_T + 22.0 { y + 20.0 } else { y - 12.0 };
            cr.set_source_rgba(fr, fg_c, fb, 0.88);
            cr.move_to(lx, ly);
            let _ = cr.show_text(&lbl);
        }
    });

    // Gesture drag for control points
    let dragging: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let gesture = gtk::GestureDrag::new();

    let cpu_p = cpu_pts.clone();
    let gpu_p = gpu_pts.clone();
    let show_p = show_gpu.clone();
    let drag_p = dragging.clone();
    let da_p = da.clone();
    gesture.connect_drag_begin(move |_, x, y| {
        let is_gpu = *show_p.borrow();
        let pts = if is_gpu {
            gpu_p.borrow()
        } else {
            cpu_p.borrow()
        };
        let aw = (da_p.width() as f64 - PAD_L - PAD_R).max(1.0);
        let ah = (da_p.height() as f64 - PAD_T - PAD_B).max(1.0);
        for (i, p) in pts.iter().enumerate() {
            let cx = PAD_L + (p.0 - 40.0) / 60.0 * aw;
            let cy = PAD_T + (1.0 - p.1 / 100.0) * ah;
            if ((cx - x).powi(2) + (cy - y).powi(2)).sqrt() < 16.0 {
                *drag_p.borrow_mut() = Some(i);
                return;
            }
        }
    });

    let cpu_u = cpu_pts.clone();
    let gpu_u = gpu_pts.clone();
    let show_u = show_gpu.clone();
    let drag_u = dragging.clone();
    let da_u = da.clone();
    gesture.connect_drag_update(move |g, _ox, oy| {
        let idx = match *drag_u.borrow() {
            Some(i) => i,
            None => return,
        };
        if let Some((_sx, sy)) = g.start_point() {
            let ah = (da_u.height() as f64 - PAD_T - PAD_B).max(1.0);
            let y = (sy + oy).clamp(PAD_T, PAD_T + ah);
            let new_speed = ((1.0 - (y - PAD_T) / ah) * 100.0).clamp(0.0, 100.0);
            let is_gpu = *show_u.borrow();
            if is_gpu {
                gpu_u.borrow_mut()[idx].1 = new_speed;
            } else {
                cpu_u.borrow_mut()[idx].1 = new_speed;
            }
            da_u.queue_draw();
        }
    });

    let drag_e = dragging.clone();
    gesture.connect_drag_end(move |_, _, _| {
        *drag_e.borrow_mut() = None;
    });
    da.add_controller(gesture);

    // CPU / GPU pill toggle
    let show_cpu_pill = show_gpu.clone();
    let da_cpu_pill = da.clone();
    cpu_pill_btn.connect_toggled(move |btn| {
        if btn.is_active() {
            *show_cpu_pill.borrow_mut() = false;
            da_cpu_pill.queue_draw();
        }
    });

    let show_gpu_pill = show_gpu.clone();
    let da_gpu_pill = da.clone();
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
        .margin_top(4)
        .build();

    let preset_name_entry = gtk::Entry::builder()
        .placeholder_text(i18n::t("preset_name"))
        .hexpand(true)
        .build();
    bottom_box.append(&preset_name_entry);

    let save_preset_btn = gtk::Button::builder().label(i18n::t("save_preset")).build();
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
    custom_btn.connect_toggled(move |b| {
        rev1.set_reveal_child(b.is_active());
    });

    // Dedicated "+ New Preset" button opens the curve editor and focuses the preset name input
    {
        let c_btn = custom_btn.clone();
        let rev_np = curve_revealer.clone();
        let entry_np = preset_name_entry.clone();
        new_preset_btn.connect_clicked(move |_| {
            if !c_btn.is_active() {
                c_btn.set_active(true);
            }
            rev_np.set_reveal_child(true);
            entry_np.grab_focus();
        });
    }

    let click_gesture = gtk::GestureClick::new();
    let rev_click = curve_revealer.clone();
    let btn_clone = custom_btn.clone();
    click_gesture.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 && btn_clone.is_active() {
            rev_click.set_reveal_child(!rev_click.reveals_child());
        }
    });
    custom_btn.add_controller(click_gesture);

    let rev2 = curve_revealer.clone();
    let cpu_pts_c = cpu_pts.clone();
    let gpu_pts_c = gpu_pts.clone();
    apply_btn.connect_clicked(move |_| {
        let pts = cpu_pts_c.borrow().clone();
        if let Ok(json) = serde_json::to_string(&pts) {
            daemon_client::save_custom_curve_sync(json);
        }
        let g_pts = gpu_pts_c.borrow().clone();
        if let Ok(json) = serde_json::to_string(&g_pts) {
            daemon_client::save_custom_gpu_curve_sync(json);
        }
        rev2.set_reveal_child(false);
    });

    let cpu_pts_save = cpu_pts.clone();
    let gpu_pts_save = gpu_pts.clone();
    let da_save = da.clone();
    let p_btns_save = preset_btns.clone();
    let fan_box_clone = fan_box.clone();
    let auto_btn_clone = auto_btn.clone();
    let preset_name_save = preset_name_entry.clone();
    let page_save = page.clone();
    save_preset_btn.connect_clicked(move |_| {
        let name = preset_name_save.text().to_string();
        if name.trim().is_empty() {
            return;
        }
        let pts = cpu_pts_save.borrow().clone();
        let preset = crate::fan_presets::FanPreset {
            name: name.clone(),
            points: pts,
        };
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
            add_preset_to_box(
                preset,
                &auto_btn_clone,
                &fan_box_clone,
                &page_save,
                &cpu_pts_save,
                &gpu_pts_save,
                &da_save,
                &p_btns_save,
            );
        }
        preset_name_save.set_text("");
    });

    let fan_box_del = fan_box.clone();
    let entry_del = preset_name_entry.clone();
    delete_btn.connect_clicked(move |_| {
        let name = entry_del.text().to_string();
        if name.trim().is_empty() {
            return;
        }
        crate::fan_presets::delete_preset(&name);

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

/// Sculpted left-aligned performance mode card with title, subtitle, active indicator, and practical description.
fn build_mode_card(
    icon_path: &str,
    title: &str,
    subtitle: &str,
    trait_text: &str,
) -> (gtk::ToggleButton, gtk::Box) {
    let btn = gtk::ToggleButton::builder()
        .css_classes(["os-mode-card"])
        .hexpand(true)
        .vexpand(true)
        .valign(gtk::Align::Fill)
        .build();

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .hexpand(true)
        .valign(gtk::Align::Start)
        .build();

    let top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .build();

    top.append(&gtk::Image::builder().file(icon_path).pixel_size(28).build());

    let titles = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    titles.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["os-mode-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    titles.append(
        &gtk::Label::builder()
            .label(subtitle)
            .css_classes(["os-mode-sub"])
            .halign(gtk::Align::Start)
            .build(),
    );
    top.append(&titles);

    let indicator = gtk::Box::builder()
        .css_classes(["os-mode-dot"])
        .valign(gtk::Align::Start)
        .halign(gtk::Align::End)
        .margin_top(2)
        .build();
    top.append(&indicator);

    inner.append(&top);

    inner.append(
        &gtk::Label::builder()
            .label(trait_text)
            .css_classes(["os-mode-trait"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .build(),
    );

    btn.set_child(Some(&inner));

    let wrap = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .valign(gtk::Align::Fill)
        .build();
    wrap.append(&btn);
    (btn, wrap)
}

/// Compact left-aligned fan control tile.
fn build_fan_card(icon_path: &str, title: &str, subtitle: &str) -> (gtk::ToggleButton, gtk::Box) {
    let btn = gtk::ToggleButton::builder()
        .css_classes(["os-fan-card"])
        .hexpand(true)
        .build();

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();

    inner.append(&gtk::Image::builder().file(icon_path).pixel_size(22).build());

    let text_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(1)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    text_col.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["os-fan-title"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    text_col.append(
        &gtk::Label::builder()
            .label(subtitle)
            .css_classes(["os-fan-sub"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(24)
            .build(),
    );
    inner.append(&text_col);
    btn.set_child(Some(&inner));

    let wrap = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .build();
    wrap.append(&btn);
    (btn, wrap)
}

#[allow(clippy::too_many_arguments)]
fn add_preset_to_box(
    preset: crate::fan_presets::FanPreset,
    auto_btn: &gtk::ToggleButton,
    fan_box: &gtk::FlowBox,
    page: &gtk::Box,
    cpu_pts: &Rc<RefCell<Vec<(f64, f64)>>>,
    gpu_pts: &Rc<RefCell<Vec<(f64, f64)>>>,
    da: &gtk::DrawingArea,
    preset_btns: &Rc<RefCell<Vec<gtk::ToggleButton>>>,
) {
    let pts_cell = Rc::new(RefCell::new(preset.points.clone()));
    let preset_icon = if preset.name.eq_ignore_ascii_case("quiet")
        || preset.name.eq_ignore_ascii_case("sessiz")
        || preset.name.eq_ignore_ascii_case("silencioso")
    {
        "fan_quiet.svg"
    } else {
        "fan_preset.svg"
    };
    let (p_btn, p_wrap) = build_fan_card(
        &crate::asset_resolver::get_asset_path(preset_icon),
        &preset.name,
        i18n::t("preset_sub"),
    );
    p_wrap.set_widget_name(&preset.name);
    p_btn.set_group(Some(auto_btn));
    preset_btns.borrow_mut().push(p_btn.clone());

    let pts_toggled = pts_cell.clone();
    let cpu_pts_toggled = cpu_pts.clone();
    let gpu_pts_toggled = gpu_pts.clone();
    let da_toggled = da.clone();
    p_btn.connect_toggled(move |btn| {
        if btn.is_active() {
            let pts = pts_toggled.borrow().clone();
            *cpu_pts_toggled.borrow_mut() = pts.clone();
            *gpu_pts_toggled.borrow_mut() = pts.clone();
            da_toggled.queue_draw();
            if let Ok(json) = serde_json::to_string(&pts) {
                daemon_client::save_custom_curve_sync(json.clone());
                daemon_client::save_custom_gpu_curve_sync(json);
            }
            daemon_client::set_fan_mode_sync("custom".to_string());
        }
    });

    let overlay = gtk::Overlay::new();
    overlay.set_widget_name(&preset.name);
    overlay.set_child(Some(&p_wrap));

    let actions_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(2)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .margin_end(6)
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
    hover.connect_enter(move |_, _, _| {
        act_clone.set_opacity(1.0);
    });
    let act_clone2 = actions_box.clone();
    hover.connect_leave(move |_| {
        act_clone2.set_opacity(0.0);
    });
    overlay.add_controller(hover);

    let win = page.clone();
    let p_name = preset.name.clone();
    let pts_edit = pts_cell.clone();
    let cpu_pts_edit = cpu_pts.clone();
    let gpu_pts_edit = gpu_pts.clone();
    let da_edit = da.clone();
    let p_btn_edit = p_btn.clone();
    let fan_box_c = fan_box.clone();
    let overlay_c = overlay.clone();
    edit_btn.connect_clicked(move |_| {
        let p_n = p_name.clone();
        let p_n1 = p_n.clone();
        let p_n2 = p_n.clone();
        let f_box = fan_box_c.clone();
        let o_lay = overlay_c.clone();
        let pts_save = pts_edit.clone();
        let cpu_pts_save2 = cpu_pts_edit.clone();
        let gpu_pts_save2 = gpu_pts_edit.clone();
        let da_save2 = da_edit.clone();
        let p_btn_save2 = p_btn_edit.clone();
        let current_pts = pts_edit.borrow().clone();
        if let Some(w) = win.root().and_downcast::<gtk::ApplicationWindow>() {
            crate::fan_curve_editor::show_fan_curve_editor(
                &w,
                &p_n,
                current_pts,
                move |new_pts| {
                    *pts_save.borrow_mut() = new_pts.clone();
                    let mut p = crate::fan_presets::load_presets();
                    if let Some(x) = p.iter_mut().find(|x| x.name == p_n1) {
                        x.points = new_pts.clone();
                        crate::fan_presets::save_presets(&p);
                    }
                    if p_btn_save2.is_active() {
                        *cpu_pts_save2.borrow_mut() = new_pts.clone();
                        *gpu_pts_save2.borrow_mut() = new_pts.clone();
                        da_save2.queue_draw();
                        if let Ok(json) = serde_json::to_string(&new_pts) {
                            daemon_client::save_custom_curve_sync(json.clone());
                            daemon_client::save_custom_gpu_curve_sync(json);
                        }
                    }
                },
                move || {
                    let p = crate::fan_presets::load_presets();
                    let p: Vec<_> = p.into_iter().filter(|x| x.name != p_n2).collect();
                    crate::fan_presets::save_presets(&p);
                    f_box.remove(&o_lay);
                },
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
