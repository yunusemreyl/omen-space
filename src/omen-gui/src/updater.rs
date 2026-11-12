use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::cell::RefCell;
use std::process::Stdio;
use std::rc::Rc;
use tokio::io::{AsyncBufReadExt, BufReader};
use crate::i18n;

/* ─────────────────────────────────────────────────────────────
   updater.rs — Overhauled OmenSpace & Firmware Update Center
   ───────────────────────────────────────────────────────────── */

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const COMPILED_GIT_SHA: &str = env!("OMEN_GIT_SHA");
const COMPILED_GIT_SHORT_SHA: &str = env!("OMEN_GIT_SHORT_SHA");
const REPO_OWNER_NAME: &str = "yunusemreyl/omen-space";

#[derive(Debug, Clone)]
struct RemoteUpdateCheck {
    installed_ver: String,
    installed_short: String,
    remote_ver: String,
    remote_short: String,
    commits_ahead: u32,
    update_available: bool,
    changelog: String,
    error: Option<String>,
}

fn resolve_installed_build() -> (String, String, String) {
    if let Ok(text) = std::fs::read_to_string("/etc/omen-space/build_info.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let ver = v["version"]
                .as_str()
                .filter(|s| !s.is_empty() && *s != "unknown")
                .unwrap_or(CURRENT_VERSION)
                .to_string();
            let sha = v["commit_sha"].as_str().unwrap_or("").trim().to_string();
            let short = v["commit_short"].as_str().unwrap_or("").trim().to_string();
            if !sha.is_empty() && sha != "unknown" {
                let s = if !short.is_empty() && short != "unknown" {
                    short
                } else {
                    sha.chars().take(7).collect()
                };
                return (ver, sha, s);
            }
        }
    }

    let sha = if COMPILED_GIT_SHA != "unknown" && !COMPILED_GIT_SHA.is_empty() {
        COMPILED_GIT_SHA.to_string()
    } else {
        "unknown".to_string()
    };
    let short = if COMPILED_GIT_SHORT_SHA != "unknown" && !COMPILED_GIT_SHORT_SHA.is_empty() {
        COMPILED_GIT_SHORT_SHA.to_string()
    } else {
        "unknown".to_string()
    };
    (CURRENT_VERSION.to_string(), sha, short)
}

fn is_newer_semver(current: &str, remote: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse::<u32>().ok())
            .collect()
    };
    let c = parse(current);
    let r = parse(remote);
    for i in 0..std::cmp::max(r.len(), c.len()) {
        let rv = *r.get(i).unwrap_or(&0);
        let cv = *c.get(i).unwrap_or(&0);
        if rv > cv {
            return true;
        }
        if cv > rv {
            return false;
        }
    }
    false
}

fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else if ch != '\r' {
            out.push(ch);
        }
    }
    out
}

async fn curl_fetch(url: &str) -> Result<String, String> {
    let out = tokio::process::Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--connect-timeout",
            "8",
            "--max-time",
            "15",
            "-H",
            "User-Agent: OmenSpace-GUI-Updater",
            url,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err(format!("HTTP request failed ({})", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

async fn check_github_updates_async() -> RemoteUpdateCheck {
    let (installed_ver, installed_sha, installed_short) = resolve_installed_build();

    let commit_url = format!(
        "https://api.github.com/repos/{}/commits/main",
        REPO_OWNER_NAME
    );
    let cargo_url = format!(
        "https://raw.githubusercontent.com/{}/main/src/omen-gui/Cargo.toml",
        REPO_OWNER_NAME
    );
    let release_url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        REPO_OWNER_NAME
    );

    let (commit_res, cargo_res, release_res) = tokio::join!(
        curl_fetch(&commit_url),
        curl_fetch(&cargo_url),
        curl_fetch(&release_url),
    );

    let valid_commit = commit_res.as_ref().ok()
        .is_some_and(|raw|omen_types::parse_commit_metadata(raw).is_ok());
    if !valid_commit {
        return RemoteUpdateCheck {
            installed_ver: installed_ver.clone(),
            installed_short,
            remote_ver: installed_ver,
            remote_short: "unknown".to_string(),
            commits_ahead: 0,
            update_available: false,
            changelog: String::new(),
            error: Some("Could not verify the upstream commit. Retry the update check.".to_string()),
        };
    }

    let mut remote_ver = installed_ver.clone();
    let mut release_notes = String::new();

    if let Ok(rel_str) = release_res {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&rel_str) {
            if let Some(tag) = v["tag_name"].as_str() {
                let clean = tag.trim_start_matches('v');
                if is_newer_semver(&remote_ver, clean) {
                    remote_ver = clean.to_string();
                }
            }
            if let Some(body) = v["body"].as_str() {
                release_notes = body.trim().to_string();
            }
        }
    }

    if let Ok(cargo_str) = cargo_res {
        for line in cargo_str.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("version") && trimmed.contains('=') {
                if let Some(ver) = trimmed.split('"').nth(1) {
                    let clean = ver.trim_start_matches('v');
                    if is_newer_semver(&remote_ver, clean) {
                        remote_ver = clean.to_string();
                    }
                    break;
                }
            }
        }
    }

    let mut remote_sha = "unknown".to_string();
    let mut remote_short = "unknown".to_string();
    let mut head_msg = String::new();

    if let Ok(c_str) = commit_res {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&c_str) {
            if let Some(sha) = v["sha"].as_str() {
                remote_sha = sha.to_string();
                remote_short = sha.chars().take(7).collect();
            }
            if let Some(msg) = v["commit"]["message"].as_str() {
                head_msg = msg.lines().next().unwrap_or("").to_string();
            }
        }
    }

    let mut commits_ahead = 0u32;
    let mut comparison_error = None;
    let mut commit_list = Vec::new();

    if installed_sha != "unknown"
        && remote_sha != "unknown"
        && installed_sha != remote_sha
    {
        let cmp_url = format!(
            "https://api.github.com/repos/{}/compare/{}...{}",
            REPO_OWNER_NAME, installed_sha, remote_sha
        );
        if let Ok(cmp_str) = curl_fetch(&cmp_url).await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cmp_str) {
                let status = v["status"].as_str().unwrap_or("");
                match omen_types::parse_comparison_ahead(&cmp_str) {
                    Ok(count) => commits_ahead=count,
                    Err(error) => comparison_error=Some(error),
                }
                if status == "diverged" && commits_ahead == 0 {
                    commits_ahead = 1;
                }
                if let Some(arr) = v["commits"].as_array() {
                    for c in arr.iter().rev().take(20) {
                        let sha_s: String = c["sha"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(7)
                            .collect();
                        let msg_s = c["commit"]["message"]
                            .as_str()
                            .unwrap_or("")
                            .lines()
                            .next()
                            .unwrap_or("");
                        if !msg_s.is_empty() {
                            commit_list.push(format!("• [{}] {}", sha_s, msg_s));
                        }
                    }
                }
            } else { comparison_error=Some("Invalid comparison response".to_string()); }
        } else {
            comparison_error=Some("Could not compare the installed and upstream commits".to_string());
        }
    }

    let ver_newer = is_newer_semver(&installed_ver, &remote_ver);
    let sha_newer = installed_sha != "unknown"
        && remote_sha != "unknown"
        && installed_sha != remote_sha
        && commits_ahead > 0;
    let update_available = ver_newer || sha_newer;

    let changelog = if !commit_list.is_empty() {
        commit_list.join("\n")
    } else if !head_msg.is_empty() && (sha_newer || release_notes.is_empty()) {
        format!("• [{}] {}", remote_short, head_msg)
    } else if !release_notes.is_empty() {
        release_notes
    } else {
        i18n::t("no_release_notes").to_string()
    };

    RemoteUpdateCheck {
        installed_ver,
        installed_short,
        remote_ver,
        remote_short,
        commits_ahead,
        update_available,
        changelog,
        error: comparison_error.or_else(|| (installed_sha == "unknown").then(|| "Installed commit is unknown; only version information could be compared.".to_string())),
    }
}

pub fn build_page(window: &adw::ApplicationWindow) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let status_page = adw::StatusPage::builder()
        .icon_name("software-update-available-symbolic")
        .title(i18n::t("title_updater"))
        .description(i18n::t("updater_desc"))
        .margin_top(18)
        .margin_bottom(8)
        .build();
    page.append(&status_page);

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .margin_start(24)
        .margin_end(24)
        .margin_bottom(32)
        .build();
    page.append(&content_box);

    // ── 1. OmenSpace Application Update Group ────────────────
    let app_group = adw::PreferencesGroup::builder()
        .title("OmenSpace")
        .build();

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
        .css_classes(["os-chip-btn", "accent"])
        .valign(gtk::Align::Center)
        .build();
    badge_box.append(&version_badge);

    let commit_badge = gtk::Label::builder()
        .label(if init_short != "unknown" {
            format!("#{}", init_short)
        } else {
            "git".to_string()
        })
        .css_classes(["os-badge"])
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
        .css_classes(["os-chip-btn", "accent"])
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
        let step_row = step_row.clone();
        let step_spinner = step_spinner.clone();
        let step_pct_lbl = step_pct_lbl.clone();
        let pbar = pbar.clone();
        let term_expander = term_expander.clone();
        let log_buffer = log_buffer.clone();
        let log_scroll = log_scroll.clone();
        let check_btn = check_btn.clone();
        let reinstall_btn = reinstall_btn.clone();
        let update_now_btn = update_now_btn.clone();
        let retry_btn = retry_btn.clone();
        let restart_app_btn = restart_app_btn.clone();
        let version_badge = version_badge.clone();
        let commit_badge = commit_badge.clone();
        let status_row = status_row.clone();
        let status_icon = status_icon.clone();

        Rc::new(move || {
            if *is_updating.borrow() {
                return;
            }
            *is_updating.borrow_mut() = true;

            check_btn.set_sensitive(false);
            reinstall_btn.set_sensitive(false);
            update_now_btn.set_sensitive(false);
            retry_btn.set_visible(false);
            restart_app_btn.set_visible(false);

            log_buffer.set_text("");
            progress_group.set_visible(true);
            progress_card.set_visible(true);
            step_spinner.set_spinning(true);
            step_row.set_title(i18n::t("updating"));
            step_row.set_subtitle(i18n::t("updater_step_1"));
            step_pct_lbl.set_label("5%");
            pbar.set_fraction(0.05);

            let is_updating_c = is_updating.clone();
            let step_row_c = step_row.clone();
            let step_spinner_c = step_spinner.clone();
            let step_pct_lbl_c = step_pct_lbl.clone();
            let pbar_c = pbar.clone();
            let term_expander_c = term_expander.clone();
            let log_buffer_c = log_buffer.clone();
            let log_scroll_c = log_scroll.clone();
            let check_btn_c = check_btn.clone();
            let reinstall_btn_c = reinstall_btn.clone();
            let update_now_btn_c = update_now_btn.clone();
            let retry_btn_c = retry_btn.clone();
            let restart_app_btn_c = restart_app_btn.clone();
            let version_badge_c = version_badge.clone();
            let commit_badge_c = commit_badge.clone();
            let status_row_c = status_row.clone();
            let status_icon_c = status_icon.clone();

            glib::spawn_future_local(async move {
                let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                let mut candidate_dirs: Vec<String> = Vec::new();
                if let Ok(p) = std::fs::read_to_string("/etc/omen-space/repo_path") {
                    let trimmed = p.trim();
                    if !trimmed.is_empty() {
                        candidate_dirs.push(trimmed.to_string());
                    }
                }
                candidate_dirs.extend([
                    format!("{}/Projeler/omen-space", home_dir),
                    format!("{}/Projects/omen-space", home_dir),
                    format!("{}/omen-space", home_dir),
                    format!("{}/Downloads/omen-space", home_dir),
                    "/var/cache/omen-space/repo".to_string(),
                    "/opt/omen-space".to_string(),
                ]);

                let valid_repo_setup = candidate_dirs
                    .into_iter()
                    .find(|dir| {
                        std::path::Path::new(&format!("{}/setup.sh", dir)).exists()
                            && std::path::Path::new(&format!("{}/Cargo.toml", dir)).exists()
                    })
                    .map(|dir| format!("{}/setup.sh", dir));

                let mut cmd_builder = tokio::process::Command::new("pkexec");
                cmd_builder
                    .arg("env")
                    .arg("OMEN_UPDATER_UI=1")
                    .arg("OMEN_NONINTERACTIVE=1");

                if let Some(setup_path) = valid_repo_setup {
                    cmd_builder
                        .arg("bash")
                        .arg(setup_path)
                        .arg("update")
                        .arg("--from-gui");
                } else if std::path::Path::new("/usr/share/omen-space/setup.sh").exists() {
                    cmd_builder
                        .arg("bash")
                        .arg("/usr/share/omen-space/setup.sh")
                        .arg("update")
                        .arg("--from-gui");
                } else {
                    let bootstrap_script = concat!(
                        "set -e; ",
                        "CACHE_REPO=/var/cache/omen-space/repo; ",
                        "mkdir -p /var/cache/omen-space /etc/omen-space; ",
                        "if [ -d \"$CACHE_REPO/.git\" ]; then ",
                        "  git -c safe.directory=\"$CACHE_REPO\" -C \"$CACHE_REPO\" fetch --all --prune && ",
                        "  git -c safe.directory=\"$CACHE_REPO\" -C \"$CACHE_REPO\" merge --ff-only origin/main; ",
                        "else ",
                        "  rm -rf \"$CACHE_REPO\" && git clone --depth 1 https://github.com/yunusemreyl/omen-space.git \"$CACHE_REPO\"; ",
                        "fi; ",
                        "echo \"$CACHE_REPO\" > /etc/omen-space/repo_path; ",
                        "exec bash \"$CACHE_REPO/setup.sh\" update --from-gui"
                    );
                    cmd_builder.arg("bash").arg("-c").arg(bootstrap_script);
                }

                let mut cmd = match cmd_builder
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        *is_updating_c.borrow_mut() = false;
                        step_spinner_c.set_spinning(false);
                        step_row_c.set_title(i18n::t("update_failed"));
                        step_row_c.set_subtitle(&format!("pkexec error: {}", e));
                        check_btn_c.set_sensitive(true);
                        reinstall_btn_c.set_sensitive(true);
                        update_now_btn_c.set_sensitive(true);
                        retry_btn_c.set_visible(true);
                        return;
                    }
                };

                let stdout = match cmd.stdout.take() {
                    Some(s) => s,
                    None => {
                        *is_updating_c.borrow_mut() = false;
                        return;
                    }
                };
                let stderr = match cmd.stderr.take() {
                    Some(s) => s,
                    None => {
                        *is_updating_c.borrow_mut() = false;
                        return;
                    }
                };

                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let tx1 = tx.clone();
                crate::daemon_client::get_runtime().spawn(async move {
                    let mut reader = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        let _ = tx1.send(line);
                    }
                });

                let tx2 = tx;
                crate::daemon_client::get_runtime().spawn(async move {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        let _ = tx2.send(line);
                    }
                });

                let buf_rx = log_buffer_c.clone();
                let scroll_rx = log_scroll_c.clone();
                let step_row_rx = step_row_c.clone();
                let step_pct_rx = step_pct_lbl_c.clone();
                let pbar_rx = pbar_c.clone();

                let log_task = glib::spawn_future_local(async move {
                    while let Some(raw_line) = rx.recv().await {
                        let clean_line = strip_ansi_codes(&raw_line);
                        if let Some(rest) = clean_line.strip_prefix("::OMEN_STEP::") {
                            let parts: Vec<&str> = rest.splitn(3, "::").collect();
                            if parts.len() == 3 {
                                let step_num = parts[0].parse::<u32>().unwrap_or(1);
                                let pct = parts[1].parse::<u32>().unwrap_or(10).clamp(0, 100);
                                pbar_rx.set_fraction(pct as f64 / 100.0);
                                step_pct_rx.set_label(&format!("{}%", pct));
                                let localized_step = match step_num {
                                    1 => i18n::t("updater_step_1"),
                                    2 => i18n::t("updater_step_2"),
                                    3 => i18n::t("updater_step_3"),
                                    4 => i18n::t("updater_step_4"),
                                    5 => i18n::t("updater_step_5"),
                                    _ => parts[2],
                                };
                                step_row_rx.set_subtitle(localized_step);
                                continue;
                            }
                        }

                        // Gently advance progress bar during cargo crate compilation lines
                        let trimmed = clean_line.trim_start();
                        if trimmed.starts_with("Compiling ") {
                            let cur = pbar_rx.fraction();
                            if (0.50..0.74).contains(&cur) {
                                let next = (cur + 0.006).min(0.74);
                                pbar_rx.set_fraction(next);
                                step_pct_rx.set_label(&format!("{}%", (next * 100.0) as u32));
                            }
                        }

                        let formatted = format!("{}\n", clean_line);
                        let mut iter = buf_rx.end_iter();
                        buf_rx.insert(&mut iter, &formatted);

                        let adj = scroll_rx.vadjustment();
                        adj.set_value(adj.upper() - adj.page_size());
                    }
                });

                let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
                crate::daemon_client::get_runtime().spawn(async move {
                    let status = cmd.wait().await;
                    let _ = wait_tx.send(status);
                });

                let status = wait_rx.await.ok().and_then(|s| s.ok());
                let _ = log_task.await;
                let ok = status.map(|s| s.success()).unwrap_or(false);

                *is_updating_c.borrow_mut() = false;
                step_spinner_c.set_spinning(false);
                check_btn_c.set_sensitive(true);
                reinstall_btn_c.set_sensitive(true);
                update_now_btn_c.set_sensitive(true);

                if ok {
                    pbar_c.set_fraction(1.0);
                    step_pct_lbl_c.set_label("100%");
                    step_row_c.set_title(i18n::t("update_completed"));
                    step_row_c.set_subtitle(i18n::t("updater_step_done"));

                    let (new_ver, _new_sha, new_short) = resolve_installed_build();
                    version_badge_c.set_label(&format!("v{}", new_ver));
                    if new_short != "unknown" {
                        commit_badge_c.set_label(&format!("#{}", new_short));
                    }
                    status_icon_c.set_icon_name(Some("object-select-symbolic"));
                    status_row_c.set_title(i18n::t("updater_up_to_date_title"));
                    status_row_c.set_subtitle(&format!(
                        "{} (v{} • #{})",
                        i18n::t("updater_up_to_date_sub"),
                        new_ver,
                        new_short
                    ));
                    update_now_btn_c.set_visible(false);
                    reinstall_btn_c.set_visible(true);
                    restart_app_btn_c.set_visible(true);
                } else {
                    step_row_c.set_title(i18n::t("update_failed"));
                    step_row_c.set_subtitle(i18n::t("updater_step_err"));
                    term_expander_c.set_expanded(true);
                    retry_btn_c.set_visible(true);
                }
            });
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
            let refresh = tokio::process::Command::new("fwupdmgr")
                .args(["refresh"])
                .output()
                .await;
            let updates = tokio::process::Command::new("fwupdmgr")
                .args(["get-updates", "--no-unreported-check"])
                .output()
                .await;
            let _ = tx.send((refresh, updates));
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
    use super::*;

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
