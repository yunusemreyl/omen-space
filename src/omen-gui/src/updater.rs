use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use crate::i18n;

/* ─────────────────────────────────────────────────────────────
   updater.rs — OmenSpace & firmware update checker
   ───────────────────────────────────────────────────────────── */

pub fn build_page(window: &adw::ApplicationWindow) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let status_page = adw::StatusPage::builder()
        .icon_name("software-update-available-symbolic")
        .title(i18n::t("title_updater"))
        .description(i18n::t("updater_desc"))
        .margin_top(24)
        .margin_bottom(12)
        .build();
    page.append(&status_page);

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_start(24)
        .margin_end(24)
        .margin_bottom(32)
        .build();
    page.append(&content_box);

    // ── OmenSpace update card ────────────────────────────────
    let app_group = adw::PreferencesGroup::builder()
        .title("OmenSpace")
        .build();

    let ver_row = adw::ActionRow::builder()
        .title(i18n::t("current_version"))
        .subtitle(i18n::t("last_checked"))
        .build();
    let app_icon = gtk::Image::builder()
        .icon_name("application-x-executable-symbolic")
        .css_classes(["accent"])
        .pixel_size(32)
        .margin_end(12)
        .build();
    ver_row.add_prefix(&app_icon);
    
    let version_badge = gtk::Label::builder()
        .label(format!("v{}", env!("CARGO_PKG_VERSION")))
        .css_classes(["os-chip-btn", "accent"])
        .valign(gtk::Align::Center)
        .build();
    ver_row.add_suffix(&version_badge);
    app_group.add(&ver_row);

    let check_row = adw::ActionRow::builder()
        .title(i18n::t("check_updates"))
        .build();
    let check_icon = gtk::Image::builder()
        .icon_name("view-refresh-symbolic")
        .pixel_size(24)
        .margin_end(12)
        .build();
    check_row.add_prefix(&check_icon);
    
    let check_btn = gtk::Button::builder()
        .label(&*i18n::t("check_updates"))
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .build();
        
    let win_clone = window.clone();
    check_btn.connect_clicked(move |_| {
        show_update_modal(&win_clone, false);
    });
    check_row.add_suffix(&check_btn);
    
    app_group.add(&check_row);

    content_box.append(&app_group);

    // ── Firmware group ────────────────────────────────────────
    let specs = crate::daemon_client::get_hardware_specs_sync();
    let fw_group = adw::PreferencesGroup::builder()
        .title(i18n::t("firmware_group"))
        .description(i18n::t("firmware_desc"))
        .build();

    let icons = ["firmware-motherboard-symbolic", "cpu-symbolic", "video-display-symbolic"];
    for (i, (device, ver)) in [
        ("HP BIOS",          specs.bios_version.as_str()),
        ("HP EC Firmware",   specs.ec_version.as_str()),
        ("NVIDIA vBIOS",     specs.vbios_version.as_str()),
    ].iter().enumerate() {
        let row = adw::ActionRow::builder().title(*device).subtitle(*ver).build();
        let icon = gtk::Image::builder()
            .icon_name(icons[i])
            .pixel_size(24)
            .margin_end(12)
            .build();
        row.add_prefix(&icon);
        fw_group.add(&row);
    }

    let fwupd_row = adw::ActionRow::builder()
        .title(i18n::t("scan_fwupd"))
        .subtitle(i18n::t("scan_fwupd_sub"))
        .build();
    let fwupd_icon = gtk::Image::builder()
        .icon_name("system-software-update-symbolic")
        .pixel_size(24)
        .margin_end(12)
        .build();
    fwupd_row.add_prefix(&fwupd_icon);
    
    let fwupd_btn = gtk::Button::builder()
        .label(&*i18n::t("scan_fwupd"))
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .build();
        
    let win_clone2 = window.clone();
    fwupd_btn.connect_clicked(move |_| {
        show_update_modal(&win_clone2, true);
    });
    fwupd_row.add_suffix(&fwupd_btn);
    
    fw_group.add(&fwupd_row);

    content_box.append(&fw_group);

    page
}

use tokio::io::{AsyncBufReadExt, BufReader};
use std::process::Stdio;

fn show_update_modal(window: &adw::ApplicationWindow, is_firmware: bool) {
    if is_firmware {
        show_firmware_update_modal(window);
    } else {
        show_app_update_modal(window);
    }
}

fn show_firmware_update_modal(window: &adw::ApplicationWindow) {
    let dialog = adw::MessageDialog::builder()
        .heading(i18n::t("scan_fwupd"))
        .body(i18n::t("checking_updates_body"))
        .transient_for(window)
        .build();

    dialog.add_response("cancel", i18n::t("cancel"));
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    dialog.set_extra_child(Some(&spinner));
    
    let dialog_clone = dialog.clone();
    glib::spawn_future_local(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        crate::daemon_client::get_runtime().spawn(async move {
            let output = tokio::process::Command::new("fwupdmgr").args(["refresh", "--force"]).output().await;
            let _ = tx.send(output);
        });

        if let Ok(output_res) = rx.await {
            let output = output_res;
            spinner.set_spinning(false);
            
            if let Ok(out) = output {
                if out.status.success() {
                    dialog_clone.set_body(i18n::t("no_updates"));
                } else {
                    dialog_clone.set_body(i18n::t("update_failed"));
                }
            } else {
                dialog_clone.set_body(i18n::t("fwupdmgr_missing"));
            }
        }
        
        dialog_clone.add_response("ok", i18n::t("ok_btn"));
        dialog_clone.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    });

    dialog.present();
}

fn show_app_update_modal(window: &adw::ApplicationWindow) {
    let dialog = gtk::Window::builder()
        .title(i18n::t("title_updater"))
        .transient_for(window)
        .modal(true)
        .default_width(700)
        .default_height(550)
        .hide_on_close(true)
        .build();

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let title_lbl = gtk::Label::builder()
        .label(i18n::t("checking_updates_title"))
        .css_classes(["title-1"])
        .build();
    vbox.append(&title_lbl);

    let spinner = gtk::Spinner::builder().spinning(true).height_request(40).build();
    vbox.append(&spinner);

    dialog.set_child(Some(&vbox));
    dialog.present();

    let dialog_clone = dialog.clone();
    let vbox_clone = vbox.clone();
    
    glib::spawn_future_local(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        crate::daemon_client::get_runtime().spawn(async move {
            let output = tokio::process::Command::new("curl")
                .args(["-s", "https://api.github.com/repos/yunusemreyl/omen-space/releases/latest"])
                .output()
                .await;
            let _ = tx.send(output);
        });

        if let Ok(output_res) = rx.await {
            spinner.set_spinning(false);
            vbox_clone.remove(&spinner);

            if let Ok(out) = output_res {
                if let Ok(json_str) = String::from_utf8(out.stdout) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let Some(tag) = parsed["tag_name"].as_str() {
                            let clean_tag = tag.trim_start_matches('v');
                            let current_ver = env!("CARGO_PKG_VERSION");
                            
                            let is_newer = |remote: &str, current: &str| -> bool {
                                let parse = |s: &str| -> Vec<u32> {
                                    s.split('.').filter_map(|p| p.parse().ok()).collect()
                                };
                                let r = parse(remote);
                                let c = parse(current);
                                for i in 0..std::cmp::max(r.len(), c.len()) {
                                    let rv = r.get(i).unwrap_or(&0);
                                    let cv = c.get(i).unwrap_or(&0);
                                    if rv > cv { return true; }
                                    if cv > rv { return false; }
                                }
                                false
                            };

                            if is_newer(clean_tag, current_ver) {
                                title_lbl.set_label(i18n::t("update_available"));
                                
                                let ver_lbl = gtk::Label::builder()
                                    .label(format!("v{} ➔ v{}", current_ver, clean_tag))
                                    .css_classes(["title-2"])
                                    .margin_bottom(8)
                                    .build();
                                vbox_clone.append(&ver_lbl);

                                let notes_lbl = gtk::Label::builder()
                                    .label(i18n::t("release_notes"))
                                    .halign(gtk::Align::Start)
                                    .build();
                                vbox_clone.append(&notes_lbl);

                                let body = parsed["body"].as_str().unwrap_or(i18n::t("no_release_notes"));
                                let buffer = gtk::TextBuffer::builder().text(body).build();
                                let text_view = gtk::TextView::builder()
                                    .buffer(&buffer)
                                    .editable(false)
                                    .wrap_mode(gtk::WrapMode::Word)
                                    .build();
                                let scroll = gtk::ScrolledWindow::builder()
                                    .child(&text_view)
                                    .min_content_height(150)
                                    .vexpand(true)
                                    .build();
                                vbox_clone.append(&scroll);

                                let hbox = gtk::Box::builder()
                                    .orientation(gtk::Orientation::Horizontal)
                                    .spacing(8)
                                    .halign(gtk::Align::End)
                                    .build();

                                let cancel_btn = gtk::Button::builder().label(i18n::t("ignore")).build();
                                let d_cancel = dialog_clone.clone();
                                cancel_btn.connect_clicked(move |_| d_cancel.close());
                                
                                let update_btn = gtk::Button::builder()
                                    .label(i18n::t("update"))
                                    .css_classes(["suggested-action"])
                                    .build();
                                
                                hbox.append(&cancel_btn);
                                hbox.append(&update_btn);
                                vbox_clone.append(&hbox);

                                // Logic for Update button
                                let v_c = vbox_clone.clone();
                                let d_c = dialog_clone.clone();
                                update_btn.connect_clicked(move |_| {
                                    start_update_process(v_c.clone(), d_c.clone());
                                });

                            } else {
                                title_lbl.set_label(i18n::t("no_updates"));
                            }
                        } else {
                            title_lbl.set_label(i18n::t("version_fetch_err"));
                        }
                    } else {
                        title_lbl.set_label(i18n::t("invalid_json_err"));
                    }
                } else {
                    title_lbl.set_label(i18n::t("connection_err"));
                }
            } else {
                title_lbl.set_label(i18n::t("connection_err"));
            }
        }
    });
}

fn start_update_process(vbox: gtk::Box, dialog: gtk::Window) {
    // Clear all children
    while let Some(child) = vbox.first_child() {
        vbox.remove(&child);
    }

    let title_lbl = gtk::Label::builder()
        .label(i18n::t("updating"))
        .css_classes(["title-1"])
        .build();
    vbox.append(&title_lbl);

    let progress = gtk::ProgressBar::builder()
        .margin_top(16)
        .margin_bottom(16)
        .build();
    progress.pulse();
    vbox.append(&progress);

    let term_toggle = gtk::ToggleButton::builder()
        .icon_name("utilities-terminal-symbolic")
        .halign(gtk::Align::Center)
        .build();
    vbox.append(&term_toggle);

    let log_buffer = gtk::TextBuffer::new(None);
    let log_view = gtk::TextView::builder()
        .buffer(&log_buffer)
        .editable(false)
        .monospace(true)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&log_view)
        .min_content_height(200)
        .vexpand(true)
        .visible(false)
        .build();
    
    let scroll_clone = scroll.clone();
    term_toggle.connect_toggled(move |t| {
        scroll_clone.set_visible(t.is_active());
    });
    
    vbox.append(&scroll);

    // Run pkexec
    let pbar_clone = progress.clone();
    let buf_clone = log_buffer.clone();
    let d_c = dialog.clone();
    
    glib::spawn_future_local(async move {
        // Pulse timer
        let pbar_timer = pbar_clone.clone();
        let pulse_source = glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            pbar_timer.pulse();
            glib::ControlFlow::Continue
        });

        let setup_path = if std::path::Path::new("/usr/share/omen-space/setup.sh").exists() {
            "/usr/share/omen-space/setup.sh".to_string()
        } else if std::path::Path::new("/opt/omen-space/setup.sh").exists() {
            "/opt/omen-space/setup.sh".to_string()
        } else {
            let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            format!("{}/omen-space/setup.sh", home_dir)
        };
        
        let mut cmd = match tokio::process::Command::new("pkexec")
            .arg(setup_path)
            .arg("update")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn() {
                Ok(c) => c,
                Err(e) => {
                    pulse_source.remove();
                    pbar_clone.set_fraction(1.0);
                    title_lbl.set_label(&format!("Update Failed: {}", e));
                    return;
                }
            };

        let stdout = if let Some(s) = cmd.stdout.take() { s } else { pulse_source.remove(); return; };
        let stderr = if let Some(s) = cmd.stderr.take() { s } else { pulse_source.remove(); return; };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let tx1 = tx.clone();
        
        crate::daemon_client::get_runtime().spawn(async move {
            let mut stdout_reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = stdout_reader.next_line().await {
                let _ = tx1.send(line);
            }
        });

        let tx2 = tx.clone();
        crate::daemon_client::get_runtime().spawn(async move {
            let mut stderr_reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = stderr_reader.next_line().await {
                let _ = tx2.send(line);
            }
        });

        let buf_rx = buf_clone.clone();
        glib::spawn_future_local(async move {
            while let Some(line) = rx.recv().await {
                let l = format!("{}\n", line);
                let mut iter = buf_rx.end_iter();
                buf_rx.insert(&mut iter, &l);
            }
        });

        let _status = cmd.wait().await;
        pulse_source.remove();

        pbar_clone.set_fraction(1.0);
        title_lbl.set_label(i18n::t("update_completed"));
        
        let close_btn = gtk::Button::builder().label(i18n::t("close")).margin_top(12).build();
        close_btn.connect_clicked(move |_| d_c.close());
        vbox.append(&close_btn);
    });
}
