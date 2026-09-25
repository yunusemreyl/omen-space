use zbus::interface;
use crate::sysmon::stats::*;
use crate::sysmon::sensors::*;
use crate::sysmon::utils::*;
#[derive(Clone)]
pub struct SysMonInterface {}

impl SysMonInterface {
    pub fn new() -> Self {
        Self {}
    }
}

#[interface(name = "org.hp.omen.SysMon")]
impl SysMonInterface {
    async fn get_diagnostics(&self) -> String {
        let stats = fetch_system_stats();
        serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string())
    }

    async fn get_hardware_specs(&self) -> String {
        let specs = get_hardware_specs();
        serde_json::to_string(&specs).unwrap_or_else(|_| "{}".to_string())
    }

    async fn generate_diagnostic_report(&self) -> String {
        let specs = get_hardware_specs();
        let stats = fetch_system_stats();
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();
        let manufacturer = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor")
            .unwrap_or_else(|_| "HP".to_string()).trim().to_string();
        let product_name = std::fs::read_to_string("/sys/class/dmi/id/product_name")
            .unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();

        let mut report = String::new();
        report.push_str("# OMENSpace Diagnostic Report\n\n");
        let date = std::process::Command::new("date")
            .arg("+%Y-%m-%d %H:%M:%S")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());
        report.push_str(&format!("> Generated: {}\n\n", date));

        // ── Environment ──────────────────────────────────────────
        report.push_str("## Environment\n\n| Field | Value |\n|-------|-------|\n");
        report.push_str(&format!("| OMENSpace version | `{}` |\n", env!("CARGO_PKG_VERSION")));
        report.push_str(&format!("| OS                | `{}` |\n", specs.os_spec));
        report.push_str(&format!("| Kernel            | `{}` |\n", specs.kernel_version));

        // ── Hardware Probe ───────────────────────────────────────
        report.push_str("\n## Hardware Probe\n\n| Field | Value |\n|-------|-------|\n");
        report.push_str(&format!("| Manufacturer | `{}` |\n", manufacturer));
        report.push_str(&format!("| Product Name | `{}` |\n", product_name));
        report.push_str(&format!("| Board ID     | `{}` |\n", board_id));
        report.push_str(&format!("| BIOS Version | `{}` |\n", specs.bios_version));
        report.push_str(&format!("| CPU          | `{}` |\n", specs.cpu_spec));
        report.push_str(&format!("| GPU          | `{}` |\n", specs.gpu_spec));
        report.push_str(&format!("| RAM          | `{}` |\n", specs.ram_spec));
        report.push_str(&format!("| NVIDIA Driver| `{}` |\n", specs.nvidia_driver));

        // ── Live Fan Telemetry ───────────────────────────────────
        report.push_str("\n## Live Fan Telemetry\n\n");
        // Show both fans individually; fall back to max if fan2 is not exposed
        if stats.fan1_rpm > 0 || stats.fan2_rpm > 0 {
            report.push_str(&format!("- CPU Fan (fan1): {} RPM (CPU Temp: {} °C)\n", stats.fan1_rpm, stats.cpu_temp));
            if stats.fan2_rpm > 0 {
                report.push_str(&format!("- GPU Fan (fan2): {} RPM (GPU Temp: {} °C)\n", stats.fan2_rpm, stats.gpu_temp));
            } else {
                report.push_str(&format!("- GPU Fan (fan2): not exposed by hwmon (GPU Temp: {} °C)\n", stats.gpu_temp));
            }
        } else {
            report.push_str(&format!("- Fan RPM: {} (individual fans not resolved — check hwmon)\n", stats.fan_rpm));
        }
        report.push_str(&format!("- CPU Power: {:.1} W  |  GPU Power: {:.1} W  |  Total: {:.1} W\n",
            stats.cpu_pwr, stats.gpu_pwr, stats.total_pwr));

        // ── hwmon Path Probe ─────────────────────────────────────
        report.push_str("\n## hwmon Sensor Paths\n\n");
        report.push_str("| hwmon | Driver | fan1_input | fan2_input | pwm1_enable |\n");
        report.push_str("|-------|--------|-----------|-----------|-------------|\n");
        if let Ok(entries) = glob::glob("/sys/class/hwmon/hwmon*") {
            for entry in entries.filter_map(Result::ok) {
                let name = std::fs::read_to_string(entry.join("name"))
                    .unwrap_or_else(|_| "?".to_string());
                let name = name.trim();
                let fan1 = std::fs::read_to_string(entry.join("fan1_input"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|_| "—".to_string());
                let fan2 = std::fs::read_to_string(entry.join("fan2_input"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|_| "—".to_string());
                let pwm1 = std::fs::read_to_string(entry.join("pwm1_enable"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|_| "—".to_string());
                let hwmon_name = entry.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if fan1 != "—" || fan2 != "—" || pwm1 != "—" {
                    report.push_str(&format!("| `{}` | `{}` | {} RPM | {} RPM | {} |\n",
                        hwmon_name, name, fan1, fan2, pwm1));
                }
            }
        }

        // ── EC Registers Snapshot ────────────────────────────────
        report.push_str("\n## EC Registers — Snapshot\n\n");

        // Try to load ec_sys with write_support so debugfs exposes the io node
        let _ = std::process::Command::new("modprobe")
            .args(["ec_sys", "write_support=1"])
            .output();
        // Also ensure debugfs is mounted if not already present
        if !std::path::Path::new("/sys/kernel/debug").exists() {
            let _ = std::process::Command::new("mount")
                .args(["-t", "debugfs", "none", "/sys/kernel/debug"])
                .output();
        }

        let debugfs_mounted = std::path::Path::new("/sys/kernel/debug").exists();
        let ec_path = "/sys/kernel/debug/ec/ec0/io";
        let ec_path_exists = std::path::Path::new(ec_path).exists();

        if let Ok(ec_data) = std::fs::read(ec_path) {
            report.push_str("```\n");
            for (i, byte) in ec_data.iter().enumerate().take(256) {
                if i % 16 == 0 {
                    report.push_str(&format!("{:02x}0 ", i / 16));
                }
                report.push_str(&format!("{:02x} ", byte));
                if i % 16 == 15 {
                    report.push('\n');
                }
            }
            report.push_str("```\n");
        } else {
            // Provide a structured troubleshooting block instead of a bare error
            report.push_str(&format!(
                "> **EC read unavailable** — run the commands below as root and regenerate.\n\
                >\n\
                > | Check | Status |\n\
                > |-------|--------|\n\
                > | Running as root | {} |\n\
                > | debugfs mounted at /sys/kernel/debug | {} |\n\
                > | ec_sys io node exists | {} |\n\
                >\n\
                > **Quick fix:**\n\
                > ```bash\n\
                > sudo modprobe ec_sys write_support=1\n\
                > sudo mount -t debugfs none /sys/kernel/debug   # if not already mounted\n\
                > ls /sys/kernel/debug/ec/ec0/io                 # should exist now\n\
                > ```\n\
                > Then regenerate this report from the OMENSpace Debug panel.\n",
                if std::env::var("USER").unwrap_or_default() == "root" || nix_is_root() { "✅ Yes" } else { "❌ No — reopen OMENSpace as root or via pkexec" },
                if debugfs_mounted { "✅ Mounted" } else { "❌ Not mounted" },
                if ec_path_exists { "✅ Present" } else { "❌ Missing (ec_sys not loaded or read_support=0)" },
            ));
        }

        // ── dmesg — hp-wmi / ACPI excerpt ───────────────────────
        report.push_str("\n## dmesg — HP WMI / ACPI Excerpt (last 30 lines)\n\n```\n");
        if let Ok(out) = std::process::Command::new("sh")
            .arg("-c")
            .arg("dmesg 2>/dev/null | grep -iE 'hp.wmi|hp_wmi|omen|AE_AML|AE_NOT_FOUND|ACPI Error' | tail -30")
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.trim().is_empty() {
                report.push_str("(no relevant hp-wmi / ACPI entries found)\n");
            } else {
                report.push_str(&text);
            }
        } else {
            report.push_str("(dmesg unavailable)\n");
        }
        report.push_str("```\n");

        report
    }

    async fn generate_rgb_issue(&self) -> String {
        let specs = get_hardware_specs();
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();
        
        let mut issue = String::new();
        issue.push_str("### Keyboard RGB Unsupported Issue\n\n");
        issue.push_str("**Product ID (Board ID):**\n");
        issue.push_str(&format!("{}\n\n", board_id));
        issue.push_str("**Full Laptop Model Name:**\n");
        issue.push_str(&format!("{}\n\n", specs.product_name));
        issue.push_str("**Kernel Version:**\n");
        issue.push_str(&format!("{}\n\n", specs.kernel_version));
        
        issue.push_str("**HID Devices (lsusb):**\n```\n");
        if let Ok(out) = std::process::Command::new("lsusb").output() {
            let out_str = String::from_utf8_lossy(&out.stdout);
            for line in out_str.lines() {
                if line.contains("Hewlett-Packard") || line.contains("HP") || line.contains("03f0") {
                    issue.push_str(line);
                    issue.push('\n');
                }
            }
        }
        issue.push_str("```\n\n");
        
        issue.push_str("**Description:**\nMy keyboard backlight is not detected or cannot be controlled by OMENSpace. Here are the diagnostics.\n");
        
        issue
    }

    #[zbus(signal)]
    pub async fn telemetry_updated(ctxt: &zbus::SignalContext<'_>, json_stats: &str) -> zbus::Result<()>;
}
