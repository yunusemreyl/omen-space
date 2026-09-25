use libadwaita as adw;
use gtk::prelude::*;
use libadwaita::prelude::AdwApplicationWindowExt;
use std::rc::Rc;

mod i18n;
mod monitoring;
mod performance_control;
mod appprofiles;
mod fan_presets;
mod fan_curve_editor;
mod undervolt;
mod mux;
mod keyboardrgb;
mod settings;
mod desktop_rgb_gui;
mod updater;
mod daemon_client;
mod asset_resolver;
const APP_ID: &str = "org.hp.OmenSpace";

fn ensure_tray_running() {
    let is_running = std::process::Command::new("pgrep")
        .arg("-x")
        .arg("omen-tray")
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);

    if !is_running {
        let spawned = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join("omen-tray")))
            .and_then(|tray_path| {
                if tray_path.exists() {
                    std::process::Command::new(tray_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .ok()
                } else {
                    None
                }
            });

        if spawned.is_none() {
            let _ = std::process::Command::new("omen-tray")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("/usr/bin/omen-tray")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                });
        }
    }
}

fn ensure_overlay_running() {
    let is_running = std::process::Command::new("pgrep")
        .arg("-x")
        .arg("omen-overlay")
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);

    if !is_running {
        let spawned = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join("omen-overlay")))
            .and_then(|overlay_path| {
                if overlay_path.exists() {
                    std::process::Command::new(overlay_path)
                        .arg("--daemon")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .ok()
                } else {
                    None
                }
            });

        if spawned.is_none() {
            let _ = std::process::Command::new("omen-overlay")
                .arg("--daemon")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("/usr/bin/omen-overlay")
                        .arg("--daemon")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                });
        }
    }
}

fn main() {
    let rt = daemon_client::get_runtime();
    let _guard = rt.enter();

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| {
        ensure_tray_running();
        ensure_overlay_running();
        adw::init().expect("Failed to initialize libadwaita");
        i18n::init();
        
        let display = gtk::gdk::Display::default().unwrap();
        let icon_theme = gtk::IconTheme::for_display(&display);
        icon_theme.add_search_path("assets");
        icon_theme.add_search_path("/usr/share/omen-space/assets");
        
        let provider = gtk::CssProvider::new();
        provider.load_from_string(include_str!("style.css"));
        let custom_provider = gtk::CssProvider::new();
        custom_provider.load_from_string(
            "navigation-split-view separator, flap separator { min-width: 0px; background: transparent; }"
        );
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &custom_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
    app.connect_activate(build_ui);
    app.run();
}

fn apply_startup_profile() {
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(sp) = json.get("startup_profile").and_then(|v| v.as_u64()) {
                    let profile_name = match sp {
                        1 => "Quiet",
                        2 => "Default",
                        3 => "Performance",
                        _ => return, // 0 = Last used, do nothing
                    };
                    crate::daemon_client::set_power_profile_sync(profile_name.to_string());
                }
            }
        }
    }
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

fn build_ui(app: &adw::Application) {
    apply_startup_profile();
    apply_appearance_mode();
    ensure_tray_running();

    if let Some(window) = app.active_window().or_else(|| app.windows().first().cloned()) {
        window.set_visible(true);
        window.present();
        return;
    }

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(i18n::t("app_title"))
        .default_width(1150)
        .default_height(820)
        .build();

    // Close button hides the window instead of killing the process (Minimize to Tray)
    window.connect_close_request(move |win| {
        ensure_tray_running();
        win.set_visible(false);
        gtk::glib::Propagation::Stop
    });

    render_ui(&window, "performance");

    // ── Unverified Board Banner ────────────────────────────────────────────
    // Check once whether the current board is in the community-verified list.
    // We read board_name directly (same source as the daemon) so the check is
    // instant and requires no D-Bus round-trip at startup.
    {
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .unwrap_or_default();
        let board_id = board_id.trim().to_uppercase();

        // Persist the "user dismissed" state in a flag file.
        let dismissed_flag = format!(
            "{}/.cache/omenspace/board_verified_dismissed_{}",
            std::env::var("HOME").unwrap_or_default(),
            board_id
        );

        let is_verified = crate::daemon_client::is_board_verified_sync(&board_id);
        let already_dismissed = std::path::Path::new(&dismissed_flag).exists();

        if !is_verified && !already_dismissed && !board_id.is_empty() {
            let issue_url = format!(
                "https://github.com/yunusemreyl/omen-space/issues/new?template=verify-laptop.yml&title=Verify+my+laptop+%28Board+{}%29",
                board_id
            );

            let banner = adw::Banner::builder()
                .title(format!(
                    "⚠️  Board {} için OMEN Space desteği henüz doğrulanmamış.",
                    board_id
                ))
                .button_label("GitHub'da Issue Oluştur")
                .revealed(true)
                .build();

            // When user clicks the action button, open the browser and dismiss
            let dismissed_flag_clone = dismissed_flag.clone();
            let issue_url_clone = issue_url.clone();
            let banner_clone = banner.clone();
            banner.connect_button_clicked(move |_| {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&issue_url_clone)
                    .spawn();
                // Mark as dismissed
                if let Some(parent) = std::path::Path::new(&dismissed_flag_clone).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&dismissed_flag_clone, "1");
                banner_clone.set_revealed(false);
            });

            // Insert banner above the main content
            if let Some(content) = window.content() {
                if let Ok(toolbar_view) = content.downcast::<adw::ToolbarView>() {
                    toolbar_view.add_top_bar(&banner);
                }
            }
        }
    }

    window.present();
}

fn render_ui(window: &adw::ApplicationWindow, initial_page: &str) {
    window.set_title(Some(i18n::t("app_title")));

    let stack = gtk::Stack::builder()
        .vexpand(true)
        .hexpand(true)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();

    // Sidebar list
    let sidebar_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .selection_mode(gtk::SelectionMode::Single)
        .build();

    let spec_header = monitoring::build_spec_header();
    let page_perf = performance_control::build_page();
    let page_mon = monitoring::build_page(true);
    let gen_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Start)
        .spacing(10)
        .margin_top(14)
        .margin_start(18)
        .margin_end(18)
        .margin_bottom(14)
        .build();
    gen_box.append(&spec_header);
    gen_box.append(&page_perf);
    gen_box.append(&page_mon);

    let m = 20;
    let page_undervolt = undervolt::build_page();
    page_undervolt.set_margin_top(m);
    page_undervolt.set_margin_start(m);
    page_undervolt.set_margin_end(m);
    page_undervolt.set_margin_bottom(m);

    let page_mux = mux::build_page();
    page_mux.set_margin_top(m);
    page_mux.set_margin_start(m);
    page_mux.set_margin_end(m);
    page_mux.set_margin_bottom(m);

    let mon_hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(12)
        .build();
    mon_hdr.append(&gtk::Label::builder().label(i18n::t("title_monitoring")).css_classes(["page-title"]).halign(gtk::Align::Start).build());
    mon_hdr.append(&gtk::Label::builder().label(i18n::t("monitoring_desc")).css_classes(["os-section-desc"]).halign(gtk::Align::Start).build());
    
    let mon_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(20)
        .margin_start(20)
        .margin_end(20)
        .margin_bottom(20)
        .build();
    mon_content.append(&mon_hdr);
    mon_content.append(&monitoring::build_page(false));

    let rgb_hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(12)
        .build();
    rgb_hdr.append(&gtk::Label::builder().label(i18n::t("title_lighting")).css_classes(["page-title"]).halign(gtk::Align::Start).build());
    rgb_hdr.append(&gtk::Label::builder().label(i18n::t("lighting_desc")).css_classes(["os-section-desc"]).halign(gtk::Align::Start).build());

    let (page_rgb_content, lb_group_opt, lb_preview_group_opt) = keyboardrgb::build_page();
    let page_rgb = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    page_rgb.append(&rgb_hdr);
    page_rgb.append(&page_rgb_content);
    
    page_rgb.set_margin_top(m);
    page_rgb.set_margin_start(m);
    page_rgb.set_margin_end(m);
    page_rgb.set_margin_bottom(m);

    // Setup dynamic on_lang_changed callback that re-renders the UI instantly
    let win_clone = window.clone();
    let on_lang_changed = Rc::new(move || {
        render_ui(&win_clone, "settings");
    });

    let page_settings = settings::build_page(window, Some(on_lang_changed), lb_group_opt, lb_preview_group_opt);
    page_settings.set_margin_top(m);
    page_settings.set_margin_start(m);
    page_settings.set_margin_end(m);
    page_settings.set_margin_bottom(m);

    let page_updater = updater::build_page(window);
    page_updater.set_margin_top(m);
    page_updater.set_margin_start(m);
    page_updater.set_margin_end(m);
    page_updater.set_margin_bottom(m);

    let page_app_profiles = appprofiles::build_page(window);
    page_app_profiles.set_margin_top(m);
    page_app_profiles.set_margin_start(m);
    page_app_profiles.set_margin_end(m);
    page_app_profiles.set_margin_bottom(m);



    stack.add_named(&gen_box, Some("performance"));
    stack.add_named(&page_undervolt, Some("undervolt"));
    stack.add_named(&page_mux, Some("mux"));
    stack.add_named(&mon_content, Some("monitoring"));
    stack.add_named(&page_rgb, Some("rgb"));
    stack.add_named(&page_app_profiles, Some("appprof"));
    stack.add_named(&page_updater, Some("updater"));
    stack.add_named(&page_settings, Some("settings"));

    stack.set_visible_child_name(initial_page);

    let toggle_sidebar_btn = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .active(true)
        .build();

    let content_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&stack)
        .build();
    content_box.append(&scroll);

    let tabs = [
        ("omen-performance-symbolic", i18n::t("nav_performance"), "performance"),
        ("omen-power-symbolic", i18n::t("nav_undervolt"), "undervolt"),
        ("omen-gpu-symbolic", i18n::t("nav_mux"), "mux"),
        ("omen-monitor-symbolic", i18n::t("nav_monitoring"), "monitoring"),
        ("omen-lighting-symbolic", i18n::t("nav_lighting"), "rgb"),
        ("omen-profiles-symbolic", i18n::t("nav_app_profiles"), "appprof"),
        ("omen-updater-symbolic", i18n::t("nav_updater"), "updater"),
    ];

    let mut sidebar_labels = Vec::new();
    
    for (icon_name, tab_name, page_name) in tabs.iter() {
        let row = gtk::ListBoxRow::builder().build();
        let box_ = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .margin_start(12)
            .margin_end(12)
            .margin_top(11)
            .margin_bottom(11)
            .build();
        box_.append(&gtk::Image::builder().icon_name(*icon_name).pixel_size(18).build());
        let label = gtk::Label::builder().label(*tab_name).margin_start(12).build();
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .reveal_child(true)
            .transition_duration(250)
            .child(&label)
            .build();
        sidebar_labels.push(revealer.clone());
        box_.append(&revealer);
        row.set_child(Some(&box_));
        row.set_widget_name(page_name);
        sidebar_list.append(&row);
    }

    let bottom_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .selection_mode(gtk::SelectionMode::Single)
        .margin_bottom(8)
        .build();
    let settings_row = gtk::ListBoxRow::builder().name("settings").build();
    let s_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .margin_start(12)
        .margin_end(12)
        .margin_top(11)
        .margin_bottom(11)
        .build();
    s_box.append(&gtk::Image::builder().icon_name("omen-settings-symbolic").pixel_size(18).build());
    let settings_label = gtk::Label::builder().label(i18n::t("nav_settings")).margin_start(12).build();
    let s_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideRight)
        .reveal_child(true)
        .transition_duration(250)
        .child(&settings_label)
        .build();
    sidebar_labels.push(s_revealer.clone());
    s_box.append(&s_revealer);
    settings_row.set_child(Some(&s_box));
    bottom_list.append(&settings_row);

    if initial_page == "settings" {
        bottom_list.select_row(Some(&settings_row));
    } else {
        let mut selected = false;
        let mut idx = 0;
        while let Some(row) = sidebar_list.row_at_index(idx) {
            if row.widget_name().as_str() == initial_page {
                sidebar_list.select_row(Some(&row));
                selected = true;
                break;
            }
            idx += 1;
        }
        if !selected {
            if let Some(first_row) = sidebar_list.row_at_index(0) {
                sidebar_list.select_row(Some(&first_row));
            }
        }
    }

    let stack_clone = stack.clone();
    let bottom_list_clone = bottom_list.clone();
    sidebar_list.connect_row_selected(move |_list, row_opt| {
        if let Some(row) = row_opt {
            bottom_list_clone.unselect_all(); // Mutual exclusion
            let page_name = row.widget_name().to_string();
            stack_clone.set_visible_child_name(&page_name);
        }
    });

    let stack_clone2 = stack.clone();
    let sidebar_list_clone2 = sidebar_list.clone();
    bottom_list.connect_row_selected(move |_list, row_opt| {
        if let Some(row) = row_opt {
            sidebar_list_clone2.unselect_all(); // Mutual exclusion
            let page_name = row.widget_name().to_string();
            stack_clone2.set_visible_child_name(&page_name);
        }
    });

    let header_logo_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .build();
    window.set_icon_name(Some("omenspace"));
    header_logo_box.append(&gtk::Image::builder().icon_name("omenspace").pixel_size(24).build());
    header_logo_box.append(&gtk::Label::builder().label("OMEN SPACE").css_classes(["title"]).build());

    let global_header = adw::HeaderBar::builder().title_widget(&header_logo_box).build();
    global_header.pack_start(&toggle_sidebar_btn);

    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-sidebar-box"])
        .build();
    sidebar_box.append(&sidebar_list);
    
    let spacer = gtk::Box::builder().vexpand(true).build();
    sidebar_box.append(&spacer);
    sidebar_box.append(&bottom_list);

    let main_hbox = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).build();
    main_hbox.append(&sidebar_box);
    main_hbox.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    
    content_box.set_hexpand(true);
    main_hbox.append(&content_box);

    let toolbar_view = adw::ToolbarView::builder()
        .content(&main_hbox)
        .build();
    toolbar_view.add_top_bar(&global_header);

    toggle_sidebar_btn.connect_toggled({
        let _sidebar_box = sidebar_box.clone();
        move |btn| {
            let is_active = btn.is_active();
            for revealer in &sidebar_labels {
                revealer.set_reveal_child(is_active);
            }
        }
    });

    window.set_content(Some(&toolbar_view));
}
