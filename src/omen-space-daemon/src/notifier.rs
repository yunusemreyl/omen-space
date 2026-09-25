use log::{info, warn};
use std::process::Command;
use glob::glob;
use zbus::Connection;

pub struct DesktopNotifier;

impl DesktopNotifier {
    pub async fn send_notification(title: &str, body: &str, urgency: u8) {
        info!("Sending desktop notification: {} - {}", title, body);
        if let Ok(connection) = Connection::session().await {
            let mut hints = std::collections::HashMap::new();
            hints.insert("urgency", zbus::zvariant::Value::U8(urgency));

            let _ = connection.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    "OMENSpace",
                    0u32,
                    "preferences-desktop-display",
                    title,
                    body,
                    Vec::<&str>::new(),
                    hints,
                    5000i32, // expire_timeout in ms
                ),
            ).await;
        } else {
            warn!("Could not connect to D-Bus session bus to send notification");
        }
    }

    /// Send an error notification (urgency=critical, icon=dialog-error, 10s timeout).
    /// Use for hardware command failures, permission errors, D-Bus method errors.
    pub async fn notify_error(context: &str, err_msg: &str) {
        let title = "OMEN Space — Hata";
        let body = format!("{}\n\nHata kodu: {}", context, err_msg);
        warn!("notify_error: {} | {}", context, err_msg);

        if let Ok(connection) = Connection::session().await {
            let mut hints = std::collections::HashMap::new();
            hints.insert("urgency", zbus::zvariant::Value::U8(2u8)); // critical

            let _ = connection.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    "OMEN Space",
                    0u32,
                    "dialog-error",
                    title,
                    body.as_str(),
                    Vec::<&str>::new(),
                    hints,
                    10000i32, // 10 seconds — errors should be readable
                ),
            ).await;
        }
    }


    /// Open path or URL in the logged in user's graphical desktop session
    pub fn open_in_user_session(target: &str) {
        info!("Opening target in user desktop session: {}", target);
        if let Ok(entries) = glob("/proc/*/environ") {
            for entry in entries.filter_map(Result::ok) {
                if let Ok(content) = std::fs::read(&entry) {
                    let env_str = String::from_utf8_lossy(&content);
                    if env_str.contains("DISPLAY=") || env_str.contains("WAYLAND_DISPLAY=") {
                        let mut display = ":0".to_string();
                        let mut user = String::new();
                        let mut dbus_addr = String::new();
                        let mut xdg_runtime = String::new();

                        for item in env_str.split('\0') {
                            if let Some((k, v)) = item.split_once('=') {
                                match k {
                                    "DISPLAY" => display = v.to_string(),
                                    "USER" | "LOGNAME" => user = v.to_string(),
                                    "DBUS_SESSION_BUS_ADDRESS" => dbus_addr = v.to_string(),
                                    "XDG_RUNTIME_DIR" => xdg_runtime = v.to_string(),
                                    _ => {}
                                }
                            }
                        }

                        // Sanitize username to prevent privilege escalation or command argument injection
                        let is_valid_username = !user.is_empty() 
                            && user != "root" 
                            && user.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

                        if is_valid_username {
                            let mut cmd = Command::new("sudo");
                            cmd.arg("-u").arg(&user).arg("env").arg(format!("DISPLAY={}", display));
                            if !dbus_addr.is_empty() {
                                cmd.arg(format!("DBUS_SESSION_BUS_ADDRESS={}", dbus_addr));
                            }
                            if !xdg_runtime.is_empty() {
                                cmd.arg(format!("XDG_RUNTIME_DIR={}", xdg_runtime));
                            }
                            cmd.args(["xdg-open", target]);
                            let _ = cmd.output();
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Open browser directly to GitHub Issue creation page with pre-filled title and body
    pub fn open_github_issue(title: &str, body: &str) {
        let repo_url = "https://github.com/yunusemreyl/omen-space/issues/new";
        let encoded_title = url_encode(title);
        let encoded_body = url_encode(body);
        let full_url = format!("{}?title={}&body={}", repo_url, encoded_title, encoded_body);
        
        info!("Launching GitHub Issue creation URL: {}", full_url);
        Self::open_in_user_session(&full_url);
    }
}

fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push_str("%20"),
            b'\n' => encoded.push_str("%0A"),
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}
