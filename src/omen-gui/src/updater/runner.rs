use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::process::Stdio;
use std::rc::Rc;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::i18n;
use omen_types::is_newer_semver;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const COMPILED_GIT_SHA: &str = env!("OMEN_GIT_SHA");
const COMPILED_GIT_SHORT_SHA: &str = env!("OMEN_GIT_SHORT_SHA");
const REPO_OWNER_NAME: &str = "yunusemreyl/omen-space";

#[derive(Debug, Clone)]
pub(super) struct RemoteUpdateCheck {
    pub(super) installed_ver: String,
    pub(super) installed_short: String,
    pub(super) remote_ver: String,
    pub(super) remote_short: String,
    pub(super) commits_ahead: u32,
    pub(super) update_available: bool,
    pub(super) changelog: String,
    pub(super) error: Option<String>,
}

pub(super) fn resolve_installed_build() -> (String, String, String) {
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

pub(super) fn strip_ansi_codes(input: &str) -> String {
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
            "--compressed",
            "-fsSL",
            "--connect-timeout",
            "8",
            "--max-time",
            "20",
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

pub(super) async fn check_github_updates_async() -> RemoteUpdateCheck {
    let (installed_ver, installed_sha, installed_short) = resolve_installed_build();

    let commit_url = format!(
        "https://api.github.com/repos/{}/commits?sha=main&per_page=30",
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

    let commit_raw = match commit_res {
        Ok(raw) => raw,
        Err(_) => {
            return RemoteUpdateCheck {
                installed_ver: installed_ver.clone(),
                installed_short,
                remote_ver: installed_ver,
                remote_short: "unknown".to_string(),
                commits_ahead: 0,
                update_available: false,
                changelog: String::new(),
                error: Some(
                    "Could not verify the upstream commit. Retry the update check.".to_string(),
                ),
            };
        }
    };

    let (remote_sha, head_msg_raw) = match omen_types::parse_commit_metadata(&commit_raw) {
        Ok(parsed) => parsed,
        Err(_) => {
            return RemoteUpdateCheck {
                installed_ver: installed_ver.clone(),
                installed_short,
                remote_ver: installed_ver,
                remote_short: "unknown".to_string(),
                commits_ahead: 0,
                update_available: false,
                changelog: String::new(),
                error: Some(
                    "Could not verify the upstream commit. Retry the update check.".to_string(),
                ),
            };
        }
    };
    let remote_short: String = remote_sha.chars().take(7).collect();
    let head_msg = head_msg_raw.lines().next().unwrap_or("").to_string();

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

    let mut commits_ahead = 0u32;
    let mut comparison_error = None;
    let mut commit_list = Vec::new();

    if installed_sha != "unknown" && remote_sha != "unknown" && installed_sha != remote_sha {
        if let Ok(Some((ahead, items))) =
            omen_types::parse_commits_list_ahead(&commit_raw, &installed_sha)
        {
            commits_ahead = ahead;
            for (sha_s, msg_s) in items.into_iter().take(20) {
                if !msg_s.is_empty() {
                    commit_list.push(format!("• [{}] {}", sha_s, msg_s));
                }
            }
        } else {
            let cmp_url = format!(
                "https://api.github.com/repos/{}/compare/{}...{}?per_page=1",
                REPO_OWNER_NAME, installed_sha, remote_sha
            );
            if let Ok(cmp_str) = curl_fetch(&cmp_url).await {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cmp_str) {
                    let status = v["status"].as_str().unwrap_or("");
                    match omen_types::parse_comparison_ahead(&cmp_str) {
                        Ok(count) => commits_ahead = count,
                        Err(error) => comparison_error = Some(error),
                    }
                    if status == "diverged" && commits_ahead == 0 {
                        commits_ahead = 1;
                    }
                    if let Some(arr) = v["commits"].as_array() {
                        for c in arr.iter().rev().take(20) {
                            let sha_s: String =
                                c["sha"].as_str().unwrap_or("").chars().take(7).collect();
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
                } else {
                    comparison_error = Some("Invalid comparison response".to_string());
                }
            } else {
                comparison_error =
                    Some("Could not compare the installed and upstream commits".to_string());
            }
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
        error: if ver_newer {
            None
        } else {
            comparison_error.or_else(|| {
                (installed_sha == "unknown").then(|| {
                    "Installed commit is unknown; only version information could be compared."
                        .to_string()
                })
            })
        },
    }
}

#[derive(Clone)]
pub(super) struct UpdateUiTargets {
    pub(super) step_row: adw::ActionRow,
    pub(super) step_spinner: gtk::Spinner,
    pub(super) step_pct_lbl: gtk::Label,
    pub(super) pbar: gtk::ProgressBar,
    pub(super) term_expander: adw::ExpanderRow,
    pub(super) log_buffer: gtk::TextBuffer,
    pub(super) log_scroll: gtk::ScrolledWindow,
    pub(super) check_btn: gtk::Button,
    pub(super) reinstall_btn: gtk::Button,
    pub(super) update_now_btn: gtk::Button,
    pub(super) retry_btn: gtk::Button,
    pub(super) restart_app_btn: gtk::Button,
    pub(super) version_badge: gtk::Label,
    pub(super) commit_badge: gtk::Label,
    pub(super) status_row: adw::ActionRow,
    pub(super) status_icon: gtk::Image,
}

/// Locate the source checkout's `setup.sh` (one that has a full source tree).
fn resolve_repo_setup_script() -> Option<String> {
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

    candidate_dirs
        .into_iter()
        .find(|dir| {
            std::path::Path::new(&format!("{}/setup.sh", dir)).exists()
                && std::path::Path::new(&format!("{}/Cargo.toml", dir)).exists()
        })
        .map(|dir| format!("{}/setup.sh", dir))
}

/// Rebuild and reload only the kernel driver via `setup.sh repair-driver`.
/// Returns Ok(()) on success or the tail of the script output on failure.
pub(super) async fn run_driver_repair() -> Result<(), String> {
    let setup = resolve_repo_setup_script()
        .filter(|p| {
            std::path::Path::new(p)
                .parent()
                .map(|d| d.join("driver/setup.sh").exists())
                .unwrap_or(false)
        })
        .ok_or_else(|| "Driver sources not found; run a full update instead.".to_string())?;

    let output = crate::daemon_client::get_runtime()
        .spawn(async move {
            tokio::process::Command::new("pkexec")
                .arg("env")
                .arg("OMEN_NONINTERACTIVE=1")
                .arg("bash")
                .arg(setup)
                .arg("repair-driver")
                .output()
                .await
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("pkexec error: {e}"))?;

    if output.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text = strip_ansi_codes(&text);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("::OMEN_STEP::"))
        .collect();
    let start = lines.len().saturating_sub(4);
    Err(lines[start..].join("\n"))
}

fn build_pkexec_update_command() -> tokio::process::Command {
    let valid_repo_setup = resolve_repo_setup_script();

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

    cmd_builder
}

fn process_log_line(
    raw_line: &str,
    buf_rx: &gtk::TextBuffer,
    scroll_rx: &gtk::ScrolledWindow,
    step_row_rx: &adw::ActionRow,
    step_pct_rx: &gtk::Label,
    pbar_rx: &gtk::ProgressBar,
) {
    let clean_line = strip_ansi_codes(raw_line);
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
            return;
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

pub(super) fn execute_update(is_updating: Rc<RefCell<bool>>, ui: UpdateUiTargets) {
    glib::spawn_future_local(async move {
        let mut cmd_builder = build_pkexec_update_command();
        let mut cmd = match cmd_builder
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                *is_updating.borrow_mut() = false;
                ui.step_spinner.set_spinning(false);
                ui.step_row.set_title(i18n::t("update_failed"));
                ui.step_row.set_subtitle(&format!("pkexec error: {}", e));
                ui.check_btn.set_sensitive(true);
                ui.reinstall_btn.set_sensitive(true);
                ui.update_now_btn.set_sensitive(true);
                ui.retry_btn.set_visible(true);
                return;
            }
        };

        let stdout = match cmd.stdout.take() {
            Some(s) => s,
            None => {
                *is_updating.borrow_mut() = false;
                return;
            }
        };
        let stderr = match cmd.stderr.take() {
            Some(s) => s,
            None => {
                *is_updating.borrow_mut() = false;
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

        let buf_rx = ui.log_buffer.clone();
        let scroll_rx = ui.log_scroll.clone();
        let step_row_rx = ui.step_row.clone();
        let step_pct_rx = ui.step_pct_lbl.clone();
        let pbar_rx = ui.pbar.clone();

        let log_task = glib::spawn_future_local(async move {
            while let Some(raw_line) = rx.recv().await {
                process_log_line(
                    &raw_line,
                    &buf_rx,
                    &scroll_rx,
                    &step_row_rx,
                    &step_pct_rx,
                    &pbar_rx,
                );
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

        *is_updating.borrow_mut() = false;
        ui.step_spinner.set_spinning(false);
        ui.check_btn.set_sensitive(true);
        ui.reinstall_btn.set_sensitive(true);
        ui.update_now_btn.set_sensitive(true);

        if ok {
            ui.pbar.set_fraction(1.0);
            ui.step_pct_lbl.set_label("100%");
            ui.step_row.set_title(i18n::t("update_completed"));
            ui.step_row.set_subtitle(i18n::t("updater_step_done"));

            let (new_ver, _new_sha, new_short) = resolve_installed_build();
            ui.version_badge.set_label(&format!("v{}", new_ver));
            if new_short != "unknown" {
                ui.commit_badge.set_label(&format!("#{}", new_short));
            }
            ui.status_icon.set_icon_name(Some("object-select-symbolic"));
            ui.status_row.set_title(i18n::t("updater_up_to_date_title"));
            ui.status_row.set_subtitle(&format!(
                "{} (v{} • #{})",
                i18n::t("updater_up_to_date_sub"),
                new_ver,
                new_short
            ));
            ui.update_now_btn.set_visible(false);
            ui.reinstall_btn.set_visible(true);
            ui.restart_app_btn.set_visible(true);
        } else {
            ui.step_row.set_title(i18n::t("update_failed"));
            ui.step_row.set_subtitle(i18n::t("updater_step_err"));
            ui.term_expander.set_expanded(true);
            ui.retry_btn.set_visible(true);
        }
    });
}

pub(super) async fn run_fwupd_check_async() -> (
    Result<std::process::Output, std::io::Error>,
    Result<std::process::Output, std::io::Error>,
) {
    let refresh = tokio::process::Command::new("fwupdmgr")
        .args(["refresh"])
        .output()
        .await;
    let updates = tokio::process::Command::new("fwupdmgr")
        .args(["get-updates", "--no-unreported-check"])
        .output()
        .await;
    (refresh, updates)
}
