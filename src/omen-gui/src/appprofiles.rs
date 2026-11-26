use crate::{daemon_client, i18n};
use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use serde::Deserialize;

/* ─────────────────────────────────────────────────────────────
appprofiles.rs — Per-application performance profiles
───────────────────────────────────────────────────────────── */

#[derive(Deserialize)]
struct AppProfile {
    process_name: String,
    power_profile: String,
    fan_mode: String,
}

pub fn build_page(window: &adw::ApplicationWindow) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    // Header
    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(4)
        .build();
    hdr.append(
        &gtk::Label::builder()
            .label(i18n::t("title_app_profiles"))
            .css_classes(["page-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    hdr.append(
        &gtk::Label::builder()
            .label(i18n::t("app_profiles_desc"))
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .build(),
    );
    page.append(&hdr);

    // ── Enable switch ─────────────────────────────────────────
    let mut init_enabled = true;
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(js) = std::fs::read_to_string(&path) {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&js) {
                if let Some(en) = j.get("app_profiles_enabled").and_then(|v| v.as_bool()) {
                    init_enabled = en;
                }
            }
        }
    }

    let enable_group = adw::PreferencesGroup::builder().build();
    let enable_row = adw::SwitchRow::builder()
        .title(i18n::t("enable_profiles"))
        .subtitle(i18n::t("enable_profiles_sub"))
        .active(init_enabled)
        .build();

    enable_row.connect_active_notify(|row| {
        let is_active = row.is_active();
        if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/.config/omenspace/settings.json", home);
            let mut json = serde_json::json!({});
            if let Ok(js) = std::fs::read_to_string(&path) {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&js) {
                    json = j;
                }
            }
            json["app_profiles_enabled"] = serde_json::json!(is_active);
            let _ = std::fs::write(
                &path,
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            );
        }
        crate::daemon_client::set_app_profiles_enabled_sync(is_active);
    });

    enable_group.add(&enable_row);
    page.append(&enable_group);

    // ── Predefined profiles (Dynamic) ─────────────────────────
    let profiles_container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    page.append(&profiles_container);

    // ── Add new profile button ────────────────────────────────
    let add_group = adw::PreferencesGroup::builder().build();
    let add_row = adw::ActionRow::builder()
        .title(i18n::t("add_profile"))
        .subtitle(i18n::t("add_profile_sub"))
        .activatable(true)
        .build();
    add_row.add_prefix(
        &gtk::Image::builder()
            .icon_name("list-add-symbolic")
            .pixel_size(20)
            .build(),
    );
    add_row.add_suffix(&gtk::Image::builder().icon_name("go-next-symbolic").build());
    add_group.add(&add_row);
    page.append(&add_group);

    let detect_group = adw::PreferencesGroup::builder()
        .title(i18n::t("detect_method"))
        .build();

    let detect_row = adw::ComboRow::builder()
        .title(i18n::t("window_detect"))
        .subtitle(i18n::t("window_detect_sub"))
        .build();
    let detect_model = gtk::StringList::new(&[
        i18n::t("proc_name"),
        i18n::t("wm_class"),
        i18n::t("app_id_wayland"),
    ]);
    detect_row.set_model(Some(&detect_model));
    detect_row.set_selected(2);
    detect_group.add(&detect_row);
    page.append(&detect_group);

    // Load initial profiles
    let container_clone = profiles_container.clone();
    glib::spawn_future_local(async move {
        reload_profiles(&container_clone).await;
    });

    let win_clone = window.clone();
    let container_clone_2 = profiles_container.clone();
    add_row.connect_activated(move |_| {
        show_add_modal(&win_clone, &container_clone_2);
    });

    page
}

async fn reload_profiles(container: &gtk::Box) {
    // Clear current children
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let group = adw::PreferencesGroup::builder()
        .title(i18n::t("defined_profiles"))
        .description(i18n::t("defined_profiles_desc"))
        .build();

    if let Ok(json) = daemon_client::get_app_profiles_async().await {
        if let Ok(profiles) = serde_json::from_str::<Vec<AppProfile>>(&json) {
            for profile in profiles {
                let row = adw::ActionRow::builder()
                    .title(&profile.process_name)
                    .subtitle(format!(
                        "{}: {}  ·  {}: {}",
                        i18n::t("profile_fmt"),
                        profile.power_profile,
                        i18n::t("fan_fmt"),
                        profile.fan_mode
                    ))
                    .build();

                let icon = match profile.process_name.as_str() {
                    "cyberpunk2077" | "cs2" | "dota2" | "eldenring" | "witcher3" | "steam" => {
                        "applications-games-symbolic"
                    }
                    "code" => "text-editor-symbolic",
                    "firefox" => "web-browser-symbolic",
                    "spotify" => "multimedia-audio-player-symbolic",
                    "blender" => "applications-graphics-symbolic",
                    _ => "application-x-executable",
                };

                row.add_prefix(
                    &gtk::Image::builder()
                        .icon_name(icon)
                        .pixel_size(24)
                        .valign(gtk::Align::Center)
                        .build(),
                );

                let perf_badge = gtk::Label::builder()
                    .label(&profile.power_profile)
                    .css_classes(if profile.power_profile == "Performance" {
                        ["badge-warn"]
                    } else if profile.power_profile == "Eco" {
                        ["badge-ok"]
                    } else {
                        ["os-section-desc"]
                    })
                    .valign(gtk::Align::Center)
                    .build();
                row.add_suffix(&perf_badge);

                let del_btn = gtk::Button::builder()
                    .icon_name("user-trash-symbolic")
                    .css_classes(["destructive-action", "circular"])
                    .valign(gtk::Align::Center)
                    .build();

                let proc_name = profile.process_name.clone();
                let c = container.clone();
                del_btn.connect_clicked(move |_| {
                    daemon_client::remove_app_profile_sync(proc_name.clone());
                    let c2 = c.clone();
                    glib::spawn_future_local(async move {
                        // Small delay to let dbus process
                        glib::timeout_future_seconds(1).await;
                        reload_profiles(&c2).await;
                    });
                });
                row.add_suffix(&del_btn);

                group.add(&row);
            }
        }
    }

    container.append(&group);
}

fn show_add_modal(window: &adw::ApplicationWindow, container: &gtk::Box) {
    let dialog = adw::MessageDialog::builder()
        .heading(i18n::t("add_profile"))
        .transient_for(window)
        .build();

    dialog.add_response("cancel", i18n::t("cancel"));
    dialog.add_response("add", i18n::t("apply"));
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("add"));
    dialog.set_close_response("cancel");

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .build();

    let entry_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let entry = gtk::Entry::builder()
        .placeholder_text("Process Name (e.g. firefox)")
        .hexpand(true)
        .build();
    entry_box.append(&entry);

    let browse_btn = gtk::Button::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text(i18n::t("browse_apps"))
        .valign(gtk::Align::Center)
        .build();
    entry_box.append(&browse_btn);
    vbox.append(&entry_box);

    let w_clone = window.clone();
    let e_clone = entry.clone();
    browse_btn.connect_clicked(move |_| {
        show_app_picker_modal(&w_clone, &e_clone);
    });

    let power_model = gtk::StringList::new(&["Eco", "Balanced", "Performance"]);
    let power_combo = adw::ComboRow::builder()
        .title(i18n::t("profile_fmt"))
        .model(&power_model)
        .build();
    let fan_model = gtk::StringList::new(&["Auto", "Max", "Custom"]);
    let fan_combo = adw::ComboRow::builder()
        .title(i18n::t("fan_fmt"))
        .model(&fan_model)
        .build();

    let pref_group = adw::PreferencesGroup::new();
    pref_group.add(&power_combo);
    pref_group.add(&fan_combo);
    vbox.append(&pref_group);

    dialog.set_extra_child(Some(&vbox));

    let c = container.clone();
    dialog.connect_response(None, move |d, response| {
        if response == "add" {
            let proc = entry.text().to_string();
            if !proc.is_empty() {
                let p_idx = power_combo.selected();
                let p_str = match p_idx {
                    0 => "Eco",
                    1 => "Balanced",
                    _ => "Performance",
                }
                .to_string();
                let f_idx = fan_combo.selected();
                let f_str = match f_idx {
                    0 => "Auto",
                    1 => "Max",
                    _ => "Custom",
                }
                .to_string();

                daemon_client::add_app_profile_sync(proc, p_str, f_str);

                let c2 = c.clone();
                glib::spawn_future_local(async move {
                    glib::timeout_future_seconds(1).await;
                    reload_profiles(&c2).await;
                });
            }
        }
        d.close();
    });

    dialog.present();
}

fn show_app_picker_modal(parent_window: &impl IsA<gtk::Window>, target_entry: &gtk::Entry) {
    let dialog = adw::MessageDialog::builder()
        .heading(i18n::t("select_app"))
        .transient_for(parent_window)
        .build();

    dialog.add_response("cancel", i18n::t("cancel"));
    dialog.set_close_response("cancel");

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .build();

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(i18n::t("search_apps"))
        .build();
    vbox.append(&search_entry);

    let listbox = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(350)
        .max_content_height(350)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(550)
        .child(&listbox)
        .build();
    vbox.append(&scroll);

    let apps = gtk::gio::AppInfo::all();
    let mut valid_apps = Vec::new();
    for app in apps {
        if app.should_show() && !app.executable().as_os_str().is_empty() {
            valid_apps.push(app);
        }
    }
    valid_apps.sort_by_key(|a| a.name().to_lowercase());

    for app in valid_apps {
        let name = app.name().to_string();
        let exec_full = app.executable().to_string_lossy().to_string();
        let exec_parts: Vec<&str> = exec_full.split_whitespace().collect();
        let bin_name = if !exec_parts.is_empty() {
            std::path::Path::new(exec_parts[0])
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            exec_full.clone()
        };

        let row = adw::ActionRow::builder()
            .title(&name)
            .subtitle(&bin_name)
            .activatable(true)
            .build();

        if let Some(icon) = app.icon() {
            let img = gtk::Image::builder()
                .icon_size(gtk::IconSize::Large)
                .valign(gtk::Align::Center)
                .build();
            img.set_from_gicon(&icon);
            row.add_prefix(&img);
        }

        row.set_widget_name(&name);

        let target = target_entry.clone();
        let d = dialog.clone();
        let bin_n = bin_name.clone();
        row.connect_activated(move |_| {
            target.set_text(&bin_n);
            d.close();
        });

        listbox.append(&row);
    }

    let search_clone = search_entry.clone();
    listbox.set_filter_func(move |row| {
        let text = search_clone.text().to_string().to_lowercase();
        if text.is_empty() {
            return true;
        }

        let action_row = if let Some(child) = row.child() {
            if let Ok(ar) = child.downcast::<adw::ActionRow>() {
                ar
            } else {
                return false;
            }
        } else {
            return false;
        };
        let title = action_row.title().to_lowercase();
        let sub = action_row.subtitle().unwrap_or_default().to_lowercase();
        title.contains(&text) || sub.contains(&text)
    });

    let lb = listbox.clone();
    search_entry.connect_search_changed(move |_| {
        lb.invalidate_filter();
    });

    dialog.set_extra_child(Some(&vbox));
    dialog.present();
}
