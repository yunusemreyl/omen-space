mod runner;

use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::process::Stdio;
use std::rc::Rc;

use crate::i18n;
use runner::{
    check_github_updates_async, execute_update, resolve_installed_build, run_fwupd_check_async,
    UpdateUiTargets,
};

/* ─────────────────────────────────────────────────────────────
updater/mod.rs — Overhauled OmenSpace & Firmware Update Center
───────────────────────────────────────────────────────────── */

pub fn build_embedded(window: &adw::ApplicationWindow) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    page.append(
        &gtk::Label::builder()
            .label(i18n::t("updater_desc"))
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .build(),
    );

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_bottom(16)
        .build();
    page.append(&content_box);

    // ── 1. OmenSpace Application Update Group ────────────────
    let app_group = adw::PreferencesGroup::builder().title("OmenSpace").build();

    let (init_ver, _init_sha, init_short) = resolve_installed_build();

    let ver_row = adw::ActionRow::builder()
        .title(i18n::t("current_version"))
        .subtitle(i18n::t("updater_installed_sub"))
        .use_markup(false)
        .build();
    let app_icon = gtk::Image::builder()
        .icon_name("application-x-executable-symbolic")
        .css_classes(["accent"])
        .pixel_size(28)
        .margin_end(12)
        .build();
    ver_row.add_prefix(&app_icon);

    let badge_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    let version_badge = gtk::Label::builder()
        .label(format!("v{}", init_ver))
        .css_classes(["badge-accent"])
        .valign(gtk::Align::Center)
        .build();
    badge_box.append(&version_badge);

    let commit_badge = gtk::Label::builder()
        .label(if init_short != "unknown" {
            format!("#{}", init_short)
        } else {
            "git".to_string()
        })
        .css_classes(["badge-neutral", "os-mono"])
        .valign(gtk::Align::Center)
        .build();
    badge_box.append(&commit_badge);

    ver_row.add_suffix(&badge_box);
    app_group.add(&ver_row);

    // Live Update Status Row
    let status_row = adw::ActionRow::builder().use_markup(false).build();
    // Set plain text after construction: ActionRow otherwise parses the initial
    // title/subtitle as markup before applying use-markup=false.
    status_row.set_title(i18n::t("checking_updates_title"));
    status_row.set_subtitle(i18n::t("updater_checking_sub"));
    let status_icon = gtk::Image::builder()
        .icon_name("emblem-synchronizing-symbolic")
        .pixel_size(24)
        .margin_end(12)
        .build();
    status_row.add_prefix(&status_icon);

    let action_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();

    let check_spinner = gtk::Spinner::builder()
        .spinning(true)
        .valign(gtk::Align::Center)
        .build();
    action_box.append(&check_spinner);

    let check_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n::t("check_updates"))
        .valign(gtk::Align::Center)
        .build();
    action_box.append(&check_btn);

    let reinstall_btn = gtk::Button::builder()
        .label(i18n::t("updater_reinstall_btn"))
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    action_box.append(&reinstall_btn);

    let update_now_btn = gtk::Button::builder()
        .label(i18n::t("updater_update_now_btn"))
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    action_box.append(&update_now_btn);

    status_row.add_suffix(&action_box);
    app_group.add(&status_row);

    // Expandable Changelog Row (shown when an update is available or commits are fetched)
    let changelog_expander = adw::ExpanderRow::builder()
        .title(i18n::t("release_notes"))
        .subtitle("GitHub main")
        .use_markup(false)
        .visible(false)
        .build();
    let changelog_icon = gtk::Image::builder()
        .icon_name("text-x-generic-symbolic")
        .pixel_size(20)
        .margin_end(8)
        .build();
    changelog_expander.add_prefix(&changelog_icon);

    let changelog_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(14)
        .margin_end(14)
        .css_classes(["monospace"])
        .build();
    changelog_expander.add_row(&changelog_label);
    app_group.add(&changelog_expander);

    content_box.append(&app_group);

    // ── 2. Inline Update Progress & Live Terminal Group ──────
    let progress_group = adw::PreferencesGroup::builder()
        .title(i18n::t("updater_progress_group"))
        .visible(false)
        .build();

    let progress_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .build();

    let step_row = adw::ActionRow::builder()
        .title(i18n::t("updating"))
        .subtitle(i18n::t("updater_step_1"))
        .use_markup(false)
        .build();
    let step_spinner = gtk::Spinner::builder()
        .spinning(false)
        .valign(gtk::Align::Center)
        .margin_end(8)
        .build();
    step_row.add_prefix(&step_spinner);

    let step_pct_lbl = gtk::Label::builder()
        .label("0%")
        .css_classes(["badge-accent", "os-mono"])
        .valign(gtk::Align::Center)
        .build();
    step_row.add_suffix(&step_pct_lbl);
    progress_group.add(&step_row);

    let pbar = gtk::ProgressBar::builder()
        .margin_top(4)
        .margin_bottom(4)
        .build();
    progress_card.append(&pbar);

    // Post-update action buttons (Restart App / Retry / Copy Log)
    let post_btn_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::End)
        .build();

    let copy_log_btn = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .label(i18n::t("updater_copy_log_btn"))
        .build();
    post_btn_box.append(&copy_log_btn);

    let retry_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .label(i18n::t("updater_retry_btn"))
        .visible(false)
        .build();
    post_btn_box.append(&retry_btn);

    let restart_app_btn = gtk::Button::builder()
        .icon_name("system-reboot-symbolic")
        .label(i18n::t("updater_restart_app_btn"))
        .css_classes(["suggested-action", "pill"])
        .visible(false)
        .build();
    post_btn_box.append(&restart_app_btn);

    progress_card.append(&post_btn_box);

    // Live Terminal Expander
    let term_expander = adw::ExpanderRow::builder()
        .title(i18n::t("updater_terminal_title"))
        .subtitle(i18n::t("updater_terminal_sub"))
        .use_markup(false)
        .expanded(false)
        .build();
    let term_icon = gtk::Image::builder()
        .icon_name("utilities-terminal-symbolic")
        .pixel_size(20)
        .margin_end(8)
        .build();
    term_expander.add_prefix(&term_icon);

    let log_buffer = gtk::TextBuffer::new(None);
    let log_view = gtk::TextView::builder()
        .buffer(&log_buffer)
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(12)
        .right_margin(12)
        .build();
    let log_scroll = gtk::ScrolledWindow::builder()
        .child(&log_view)
        .min_content_height(240)
        .max_content_height(360)
        .vexpand(true)
        .build();
    term_expander.add_row(&log_scroll);

    progress_group.add(&term_expander);
    content_box.append(&progress_group);
    content_box.append(&progress_card);
    progress_card.set_visible(false);

    // Copy log handler
    {
        let buf_c = log_buffer.clone();
        let copy_btn_c = copy_log_btn.clone();
        copy_log_btn.connect_clicked(move |_| {
            let text = buf_c
                .text(&buf_c.start_iter(), &buf_c.end_iter(), false)
                .to_string();
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&text);
                copy_btn_c.set_label(i18n::t("updater_copied"));
                let btn_reset = copy_btn_c.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    btn_reset.set_label(i18n::t("updater_copy_log_btn"));
                });
            }
        });
    }

    // Restart Application handler
    {
        restart_app_btn.connect_clicked(move |_| {
            let exe = if std::path::Path::new("/usr/bin/omen-gui").exists() {
                std::path::PathBuf::from("/usr/bin/omen-gui")
            } else {
                std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("omen-gui"))
            };
            let _ = std::process::Command::new(exe)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            std::process::exit(0);
        });
    }

    // Shared guard so multiple update/check tasks don't overlap
    let is_updating = Rc::new(RefCell::new(false));

    // Helper closure to run the GitHub check and update UI
    let run_check: Rc<dyn Fn()> = {
        let status_row = status_row.clone();
        let status_icon = status_icon.clone();
        let check_spinner = check_spinner.clone();
        let check_btn = check_btn.clone();
        let reinstall_btn = reinstall_btn.clone();
        let update_now_btn = update_now_btn.clone();
        let changelog_expander = changelog_expander.clone();
        let changelog_label = changelog_label.clone();
        let version_badge = version_badge.clone();
        let commit_badge = commit_badge.clone();
        let is_updating = is_updating.clone();

        Rc::new(move || {
            if *is_updating.borrow() {
                return;
            }
            check_spinner.set_visible(true);
            check_spinner.set_spinning(true);
            check_btn.set_sensitive(false);
            status_row.set_title(i18n::t("checking_updates_title"));
            status_row.set_subtitle(i18n::t("updater_checking_sub"));
            status_icon.set_icon_name(Some("emblem-synchronizing-symbolic"));

            let status_row_c = status_row.clone();
            let status_icon_c = status_icon.clone();
            let check_spinner_c = check_spinner.clone();
            let check_btn_c = check_btn.clone();
            let reinstall_btn_c = reinstall_btn.clone();
            let update_now_btn_c = update_now_btn.clone();
            let changelog_expander_c = changelog_expander.clone();
            let changelog_label_c = changelog_label.clone();
            let version_badge_c = version_badge.clone();
            let commit_badge_c = commit_badge.clone();

            glib::spawn_future_local(async move {
                let (tx, rx) = tokio::sync::oneshot::channel();
                crate::daemon_client::get_runtime().spawn(async move {
                    let res = check_github_updates_async().await;
                    let _ = tx.send(res);
                });

                if let Ok(res) = rx.await {
                    check_spinner_c.set_spinning(false);
                    check_spinner_c.set_visible(false);
                    check_btn_c.set_sensitive(true);

                    version_badge_c.set_label(&format!("v{}", res.installed_ver));
                    if res.installed_short != "unknown" {
                        commit_badge_c.set_label(&format!("#{}", res.installed_short));
                    }

                    if let Some(err) = res.error {
                        status_icon_c.set_icon_name(Some("dialog-warning-symbolic"));
                        status_row_c.set_title(i18n::t("update_failed"));
                        status_row_c.set_subtitle(&err);
                        reinstall_btn_c.set_visible(true);
                        update_now_btn_c.set_visible(false);
                        return;
                    }

                    if res.update_available {
                        status_icon_c.set_icon_name(Some("software-update-available-symbolic"));
                        let title = if res.commits_ahead > 0 {
                            format!(
                                "{} (v{} • #{} — {} {})",
                                i18n::t("updater_available_title"),
                                res.remote_ver,
                                res.remote_short,
                                res.commits_ahead,
                                i18n::t("updater_commits_ahead")
                            )
                        } else {
                            format!(
                                "{} (v{} ➔ v{})",
                                i18n::t("updater_available_title"),
                                res.installed_ver,
                                res.remote_ver
                            )
                        };
                        status_row_c.set_title(&title);
                        status_row_c.set_subtitle(i18n::t("updater_available_sub"));
                        update_now_btn_c.set_visible(true);
                        reinstall_btn_c.set_visible(false);

                        changelog_label_c.set_label(&res.changelog);
                        changelog_expander_c.set_subtitle(&format!(
                            "v{} (#{}) ➔ v{} (#{})",
                            res.installed_ver,
                            res.installed_short,
                            res.remote_ver,
                            res.remote_short
                        ));
                        changelog_expander_c.set_visible(true);
                        changelog_expander_c.set_expanded(true);
                    } else {
                        status_icon_c.set_icon_name(Some("object-select-symbolic"));
                        status_row_c.set_title(i18n::t("updater_up_to_date_title"));
                        status_row_c.set_subtitle(&format!(
                            "{} (v{} • #{})",
                            i18n::t("updater_up_to_date_sub"),
                            res.installed_ver,
                            res.remote_short
                        ));
                        update_now_btn_c.set_visible(false);
                        reinstall_btn_c.set_visible(true);

                        if !res.changelog.is_empty() {
                            changelog_label_c.set_label(&res.changelog);
                            changelog_expander_c.set_subtitle(&format!(
                                "v{} • #{}",
                                res.installed_ver, res.remote_short
                            ));
                            changelog_expander_c.set_visible(true);
                            changelog_expander_c.set_expanded(false);
                        }
                    }
                }
            });
        })
    };

    // Helper closure to execute the update via pkexec + setup.sh
    let run_update: Rc<dyn Fn()> = {
        let is_updating = is_updating.clone();
        let progress_group = progress_group.clone();
        let progress_card = progress_card.clone();
        let ui = UpdateUiTargets {
            step_row,
            step_spinner,
            step_pct_lbl,
            pbar,
            term_expander,
            log_buffer,
            log_scroll,
            check_btn: check_btn.clone(),
            reinstall_btn: reinstall_btn.clone(),
            update_now_btn: update_now_btn.clone(),
            retry_btn: retry_btn.clone(),
            restart_app_btn,
            version_badge,
            commit_badge,
            status_row,
            status_icon,
        };

        Rc::new(move || {
            if *is_updating.borrow() {
                return;
            }
            *is_updating.borrow_mut() = true;

            ui.check_btn.set_sensitive(false);
            ui.reinstall_btn.set_sensitive(false);
            ui.update_now_btn.set_sensitive(false);
            ui.retry_btn.set_visible(false);
            ui.restart_app_btn.set_visible(false);

            ui.log_buffer.set_text("");
            progress_group.set_visible(true);
            progress_card.set_visible(true);
            ui.step_spinner.set_spinning(true);
            ui.step_row.set_title(i18n::t("updating"));
            ui.step_row.set_subtitle(i18n::t("updater_step_1"));
            ui.step_pct_lbl.set_label("5%");
            ui.pbar.set_fraction(0.05);

            execute_update(is_updating.clone(), ui.clone());
        })
    };

    // Wire buttons
    {
        let rc = run_check.clone();
        check_btn.connect_clicked(move |_| rc());
    }
    {
        let ru = run_update.clone();
        update_now_btn.connect_clicked(move |_| ru());
    }
    {
        let ru = run_update.clone();
        reinstall_btn.connect_clicked(move |_| ru());
    }
    {
        let ru = run_update.clone();
        retry_btn.connect_clicked(move |_| ru());
    }

    // Trigger initial non-blocking update check automatically
    {
        let rc = run_check.clone();
        glib::idle_add_local_once(move || {
            rc();
        });
    }

    // ── 3. Firmware & Kernel Driver Group ────────────────────
    let specs = crate::daemon_client::get_hardware_specs_sync();
    let fw_group = adw::PreferencesGroup::builder()
        .title(i18n::t("firmware_group"))
        .description(i18n::t("firmware_desc"))
        .build();

    // Live DKMS driver status row
    let dkms_loaded = std::path::Path::new("/sys/module/hp_wmi").exists()
        && std::path::Path::new("/sys/module/hp_omen_extra").exists();
    let dkms_row = adw::ActionRow::builder().use_markup(false).build();
    dkms_row.set_title(i18n::t("updater_dkms_driver"));
    dkms_row.set_subtitle(if dkms_loaded {
        i18n::t("updater_dkms_loaded")
    } else {
        i18n::t("updater_dkms_missing")
    });
    let dkms_icon = gtk::Image::builder()
        .icon_name(if dkms_loaded {
            "object-select-symbolic"
        } else {
            "dialog-warning-symbolic"
        })
        .pixel_size(24)
        .margin_end(12)
        .build();
    dkms_row.add_prefix(&dkms_icon);
    fw_group.add(&dkms_row);

    let icons = [
        "firmware-motherboard-symbolic",
        "cpu-symbolic",
        "video-display-symbolic",
    ];
    for (i, (device, ver)) in [
        ("HP BIOS", specs.bios_version.as_str()),
        ("HP EC Firmware", specs.ec_version.as_str()),
        ("NVIDIA vBIOS", specs.vbios_version.as_str()),
    ]
    .iter()
    .enumerate()
    {
        let row = adw::ActionRow::builder()
            .title(*device)
            .subtitle(*ver)
            .build();
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
        .label(i18n::t("scan_fwupd"))
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .build();

    let win_clone = window.clone();
    fwupd_btn.connect_clicked(move |_| {
        show_firmware_update_modal(&win_clone);
    });
    fwupd_row.add_suffix(&fwupd_btn);

    fw_group.add(&fwupd_row);
    content_box.append(&fw_group);

    page
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
            let res = run_fwupd_check_async().await;
            let _ = tx.send(res);
        });

        if let Ok((refresh_res, updates_res)) = rx.await {
            spinner.set_spinning(false);
            spinner.set_visible(false);

            match (refresh_res, updates_res) {
                (Err(_), _) | (_, Err(_)) => {
                    dialog_clone.set_body(i18n::t("fwupdmgr_missing"));
                }
                (Ok(_), Ok(upd)) => {
                    let stdout = String::from_utf8_lossy(&upd.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&upd.stderr).trim().to_string();
                    // Exit code 2 from fwupdmgr get-updates means "No updates available"
                    if upd.status.code() == Some(2)
                        || stdout.to_lowercase().contains("no updates")
                        || stderr.to_lowercase().contains("no updates")
                        || (upd.status.success() && stdout.is_empty())
                    {
                        dialog_clone.set_body(i18n::t("no_updates"));
                    } else if upd.status.success() && !stdout.is_empty() {
                        dialog_clone.set_body(&stdout);
                    } else {
                        dialog_clone.set_body(i18n::t("no_updates"));
                    }
                }
            }
        }

        dialog_clone.add_response("ok", i18n::t("ok_btn"));
        dialog_clone.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    });

    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::runner::strip_ansi_codes;
    use omen_types::is_newer_semver;

    #[test]
    fn test_strip_ansi_codes() {
        let raw = "\x1b[0;32m[OK]\x1b[0m Custom hp-wmi loaded\r";
        assert_eq!(strip_ansi_codes(raw), "[OK] Custom hp-wmi loaded");
    }

    #[test]
    fn test_is_newer_semver() {
        assert!(is_newer_semver("2.1.7", "2.1.8"));
        assert!(!is_newer_semver("2.1.8", "2.1.7"));
        assert!(!is_newer_semver("2.1.8", "2.1.8"));
    }
}
