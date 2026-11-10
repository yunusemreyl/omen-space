use serde::{Deserialize, Serialize};
use log::{info, warn};
use crate::notifier::DesktopNotifier;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMPILED_GIT_SHA: &str = env!("OMEN_GIT_SHA");
pub const COMPILED_GIT_SHORT_SHA: &str = env!("OMEN_GIT_SHORT_SHA");
pub const REPO_OWNER_NAME: &str = "yunusemreyl/omen-space";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub installed_commit: String,
    pub latest_commit: String,
    pub commits_behind: u32,
    pub update_available: bool,
    pub release_notes: String,
    pub release_url: String,
    pub download_url: String,
    pub status_message: String,
    #[serde(default)]
    pub error: Option<String>,
}

pub struct AutoUpdateService;

impl AutoUpdateService {
    pub fn resolve_installed_commit() -> (String, String) {
        if let Ok(text) = std::fs::read_to_string("/etc/omen-space/build_info.json") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let sha = v["commit_sha"].as_str().unwrap_or("").trim().to_string();
                let short = v["commit_short"].as_str().unwrap_or("").trim().to_string();
                if !sha.is_empty() && sha != "unknown" {
                    let s = if !short.is_empty() && short != "unknown" {
                        short
                    } else {
                        sha.chars().take(7).collect()
                    };
                    return (sha, s);
                }
            }
        }
        if COMPILED_GIT_SHA != "unknown" && !COMPILED_GIT_SHA.is_empty() {
            return (
                COMPILED_GIT_SHA.to_string(),
                COMPILED_GIT_SHORT_SHA.to_string(),
            );
        }
        ("unknown".to_string(), "unknown".to_string())
    }

    pub async fn check_for_updates() -> AppUpdateInfo {
        let (installed_sha, installed_short) = Self::resolve_installed_commit();
        info!(
            "Omen Space: Checking GitHub for application updates (Current: v{}, commit: {})...",
            CURRENT_VERSION, installed_short
        );

        let (
            latest_version,
            latest_sha,
            latest_short,
            commits_behind,
            notes,
            release_url,
            download_url,
            metadata_error,
        ) = fetch_github_update_metadata(&installed_sha).await;

        let semver_newer = is_newer_semver(CURRENT_VERSION, &latest_version);
        let commit_newer = installed_sha != "unknown"
            && latest_sha != "unknown"
            && installed_sha != latest_sha
            && commits_behind > 0;

        let error = if metadata_error.is_some() { metadata_error } else if latest_sha == "unknown" {
            Some("Could not verify the upstream commit. Retry the update check.".to_string())
        } else if installed_sha == "unknown" {
            Some("Installed commit is unknown; only version information could be compared.".to_string())
        } else { None };
        let update_available = error.is_none() && (semver_newer || commit_newer);

        let status_message = if let Some(error) = &error { error.clone() } else if update_available {
            if semver_newer {
                format!(
                    "New Omen Space version v{} ({}) available! (Current: v{} • {})",
                    latest_version.trim_start_matches('v'),
                    latest_short,
                    CURRENT_VERSION,
                    installed_short
                )
            } else {
                format!(
                    "New Omen Space build ({}, {} commit(s) ahead) available on main! (Current: v{} • {})",
                    latest_short, commits_behind, CURRENT_VERSION, installed_short
                )
            }
        } else {
            format!(
                "Omen Space is up to date (v{} • {})",
                CURRENT_VERSION, installed_short
            )
        };

        info!("Omen Space Update Check: {}", status_message);

        if update_available {
            DesktopNotifier::send_notification_tagged(
                Some("omenspace-app-update"),
                "Omen Space Update Available",
                &status_message,
                1,
            )
            .await;
        }

        AppUpdateInfo {
            current_version: CURRENT_VERSION.to_string(),
            latest_version,
            installed_commit: installed_short,
            latest_commit: latest_short,
            commits_behind,
            update_available,
            release_notes: notes,
            release_url,
            download_url,
            status_message,
            error,
        }
    }

    pub async fn apply_update() -> String {
        info!("Starting Omen Space application update...");
        let info = Self::check_for_updates().await;

        DesktopNotifier::send_notification_tagged(
            Some("omenspace-app-update"),
            "Omen Space Updating",
            &format!(
                "Updating Omen Space to {} ({})...",
                info.latest_version, info.latest_commit
            ),
            1,
        )
        .await;

        // Locate setup.sh from configured repo_path, cache, or /usr/share/omen-space/setup.sh
        let mut setup_script = "/usr/share/omen-space/setup.sh".to_string();
        if let Ok(repo_path) = std::fs::read_to_string("/etc/omen-space/repo_path") {
            let trimmed = repo_path.trim();
            let candidate = format!("{}/setup.sh", trimmed);
            if !trimmed.is_empty() && std::path::Path::new(&candidate).exists() {
                setup_script = candidate;
            }
        }

        // Launch inside a transient systemd unit so `systemctl stop omen-space-daemon.service`
        // during `do_install` does NOT terminate the update process itself!
        let has_systemd_run = tokio::process::Command::new("systemd-run")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_systemd_run {
            let _ = tokio::process::Command::new("systemctl")
                .args(["reset-failed", "omen-space-updater.service"])
                .output()
                .await;

            let spawn_res = tokio::process::Command::new("systemd-run")
                .args([
                    "--unit=omen-space-updater",
                    "--description=OmenSpace Self-Updater",
                    "--collect",
                    "--setenv=OMEN_NONINTERACTIVE=1",
                    "/usr/bin/env",
                    "bash",
                    &setup_script,
                    "update",
                    "--non-interactive",
                ])
                .output()
                .await;

            if let Ok(out) = spawn_res {
                if out.status.success() {
                    return serde_json::json!({
                        "success": true,
                        "version": info.latest_version,
                        "commit": info.latest_commit,
                        "message": "Update started in background unit 'omen-space-updater.service'. Follow progress with: journalctl -u omen-space-updater -f"
                    })
                    .to_string();
                }
            }
        }

        warn!("systemd-run unavailable or failed; spawning detached updater process...");
        let log_file = "/var/log/omen-space-update.log";
        let detached_cmd = format!(
            "nohup env OMEN_NONINTERACTIVE=1 bash '{}' update --non-interactive > '{}' 2>&1 &",
            setup_script.replace('\'', "'\\''"),
            log_file
        );
        let res = tokio::process::Command::new("bash")
            .args(["-c", &detached_cmd])
            .status()
            .await;

        match res {
            Ok(s) if s.success() => serde_json::json!({
                "success": true,
                "version": info.latest_version,
                "commit": info.latest_commit,
                "message": format!("Update started in background. Follow progress in {}", log_file)
            })
            .to_string(),
            _ => serde_json::json!({
                "success": false,
                "message": "Failed to launch background updater process."
            })
            .to_string(),
        }
    }
}

async fn curl_get(url: &str) -> Option<String> {
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
            "User-Agent: OmenSpace-Updater",
            url,
        ])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

async fn fetch_github_update_metadata(
    installed_sha: &str,
) -> (String, String, String, u32, String, String, String, Option<String>) {
    let release_api = format!(
        "https://api.github.com/repos/{}/releases/latest",
        REPO_OWNER_NAME
    );
    let commit_api = format!(
        "https://api.github.com/repos/{}/commits/main",
        REPO_OWNER_NAME
    );
    let raw_cargo_url = format!(
        "https://raw.githubusercontent.com/{}/main/src/omen-gui/Cargo.toml",
        REPO_OWNER_NAME
    );

    let (release_res, commit_res, cargo_res) = tokio::join!(
        curl_get(&release_api),
        curl_get(&commit_api),
        curl_get(&raw_cargo_url),
    );

    let mut latest_version = format!("v{}", CURRENT_VERSION);
    let mut release_notes = String::new();
    let mut html_url = format!("https://github.com/{}/releases", REPO_OWNER_NAME);

    if let Some(rel_json) = release_res {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&rel_json) {
            if let Some(tag) = v["tag_name"].as_str() {
                if is_newer_semver(&latest_version, tag) {
                    latest_version = if tag.starts_with('v') {
                        tag.to_string()
                    } else {
                        format!("v{}", tag)
                    };
                }
            }
            if let Some(body) = v["body"].as_str() {
                release_notes = body.to_string();
            }
            if let Some(url) = v["html_url"].as_str() {
                html_url = url.to_string();
            }
        }
    }

    if let Some(cargo_toml) = cargo_res {
        for line in cargo_toml.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("version") && trimmed.contains('=') {
                if let Some(ver) = trimmed.split('"').nth(1) {
                    if is_newer_semver(&latest_version, ver) {
                        latest_version = format!("v{}", ver.trim_start_matches('v'));
                    }
                    break;
                }
            }
        }
    }

    let mut latest_sha = "unknown".to_string();
    let mut latest_short = "unknown".to_string();
    let mut latest_commit_msg = String::new();

    if let Some(c_json) = commit_res {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&c_json) {
            if let Some(sha) = v["sha"].as_str().filter(|sha|omen_types::is_commit_sha(sha)) {
                latest_sha = sha.to_string();
                latest_short = sha.chars().take(7).collect();
            }
            if let Some(msg) = v["commit"]["message"].as_str() {
                latest_commit_msg = msg.lines().next().unwrap_or("").to_string();
            }
        }
    }

    let mut commits_behind = 0u32;
    let mut comparison_error = None;
    let mut commit_changelog = String::new();
    if installed_sha != "unknown"
        && latest_sha != "unknown"
        && installed_sha != latest_sha
    {
        let compare_url = format!(
            "https://api.github.com/repos/{}/compare/{}...{}",
            REPO_OWNER_NAME, installed_sha, latest_sha
        );
        if let Some(cmp_json) = curl_get(&compare_url).await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cmp_json) {
                match omen_types::parse_comparison_ahead(&cmp_json) {
                    Ok(count) => commits_behind=count,
                    Err(error) => comparison_error=Some(error),
                }
                if let Some(commits) = v["commits"].as_array() {
                    let mut lines = Vec::new();
                    for c in commits.iter().rev().take(15) {
                        let c_sha: String = c["sha"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(7)
                            .collect();
                        let c_msg = c["commit"]["message"]
                            .as_str()
                            .unwrap_or("")
                            .lines()
                            .next()
                            .unwrap_or("");
                        if !c_msg.is_empty() {
                            lines.push(format!("• [{}] {}", c_sha, c_msg));
                        }
                    }
                    if !lines.is_empty() {
                        commit_changelog = lines.join("\n");
                    }
                }
            } else { comparison_error=Some("Invalid comparison response".to_string()); }
        } else {
            comparison_error=Some("Could not compare the installed and upstream commits".to_string());
        }
    }

    let combined_notes = if !commit_changelog.is_empty() {
        commit_changelog
    } else if !latest_commit_msg.is_empty() && release_notes.is_empty() {
        format!("• [{}] {}", latest_short, latest_commit_msg)
    } else if !release_notes.is_empty() {
        release_notes
    } else {
        "No release notes available.".to_string()
    };

    let download_url = format!(
        "https://github.com/{}/archive/refs/heads/main.tar.gz",
        REPO_OWNER_NAME
    );

    (
        latest_version,
        latest_sha,
        latest_short,
        commits_behind,
        combined_notes,
        html_url,
        download_url,
        comparison_error,
    )
}

fn is_newer_semver(current: &str, remote: &str) -> bool {
    let clean_current = current.trim_start_matches('v');
    let clean_remote = remote.trim_start_matches('v');
    let parse_ver = |s: &str| -> (u32, u32, u32) {
        let parts: Vec<u32> = s
            .split('.')
            .filter_map(|p| p.parse::<u32>().ok())
            .collect();
        (
            *parts.first().unwrap_or(&0),
            *parts.get(1).unwrap_or(&0),
            *parts.get(2).unwrap_or(&0),
        )
    };

    let c = parse_ver(clean_current);
    let r = parse_ver(clean_remote);

    r > c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_semver() {
        assert!(is_newer_semver("2.1.7", "v2.1.8"));
        assert!(is_newer_semver("v2.1.7", "2.1.8"));
        assert!(!is_newer_semver("2.1.8", "v2.1.7"));
        assert!(!is_newer_semver("2.1.8", "v2.1.8"));
    }
}
