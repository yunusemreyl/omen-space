import re

with open("src/omen-gui/src/monitoring.rs", "r") as f:
    content = f.read()

# Replace from "    // ── Daemon Offline Banner ─" to before "// 2. Main monitor row:"
start_str = "    // ── Daemon Offline Banner ─"
end_str = "    // 2. Main monitor row: CPU + GPU + Device"

start_idx = content.find(start_str)
end_idx = content.find(end_str)

if start_idx == -1 or end_idx == -1:
    print("Cannot find banner section")
    exit(1)

new_code = """
    // ── Daemon Offline Dialog ────────────────────────────────────────────────
    use libadwaita as adw;
    use libadwaita::prelude::*;

    let sec_clone = sec.clone();
    crate::daemon_client::subscribe_daemon_status(move |status| {
        use crate::daemon_client::DaemonStatus;
        if status == DaemonStatus::Offline {
            if let Some(win) = sec_clone.root().and_downcast::<gtk::Window>() {
                let dialog = adw::MessageDialog::new(
                    Some(&win),
                    "Daemon Çalışmıyor",
                    "Daemon şu an çalışmıyor. Fan RPM'leri alınamıyor.\\nDaemonu yeniden başlatmak ister misiniz?"
                );
                dialog.add_response("cancel", "İptal");
                dialog.add_response("restart", "Yeniden Başlat");
                dialog.set_response_appearance("restart", adw::ResponseAppearance::Suggested);
                
                dialog.connect_response(None, |d, response| {
                    if response == "restart" {
                        let spawned = std::process::Command::new("pkexec")
                            .args(["systemctl", "restart", "omen-space-daemon"])
                            .spawn();
                        if spawned.is_err() {
                            let _ = std::process::Command::new("systemctl")
                                .args(["restart", "omen-space-daemon"])
                                .spawn();
                        }
                    }
                    d.close();
                });
                dialog.present();
            }
        }
    });

"""

final_content = content[:start_idx] + new_code + content[end_idx:]

with open("src/omen-gui/src/monitoring.rs", "w") as f:
    f.write(final_content)

print("Patched banner to dialog!")
