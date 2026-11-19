use zbus::interface;
use crate::sysmon::stats::*;
use crate::sysmon::sensors::*;
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
        let mut specs = get_hardware_specs();
        // The rest of the specs never change at runtime and stay cached (they cost a
        // nvidia-smi/lspci spawn); the drive list is a few sysfs reads, so keep it fresh.
        crate::sysmon::drives::refresh_specs(&mut specs);
        if let Some(bat) = crate::sysmon::battery::current_battery() {
            let cap_wh = bat.energy_full_wh.or(bat.energy_design_wh).unwrap_or(0.0);
            let health_str = bat.health_pct.map(|h| format!(" · {:.0}% Health", h)).unwrap_or_default();
            let cycles_str = bat.cycle_count.map(|c| format!(" ({} cycles)", c)).unwrap_or_default();
            if cap_wh > 0.0 {
                specs.battery_spec = format!("{:.1} Wh Li-ion{}{}", cap_wh, health_str, cycles_str);
            } else {
                specs.battery_spec = format!("{}%{}", bat.charge_pct, health_str);
            }
        }
        serde_json::to_string(&specs).unwrap_or_else(|_| "{}".to_string())
    }

    async fn generate_diagnostic_report(&self) -> String {
        // Run all sysfs/command collection on a blocking worker thread so the D-Bus
        // reactor and GUI never freeze during report generation (#364).
        tokio::task::spawn_blocking(build_unified_system_dump)
            .await
            .unwrap_or_else(|e| format!("# OMENSpace Full System Dump\n\nError collecting report: {}", e))
    }

    async fn generate_rgb_issue(&self) -> String {
        tokio::task::spawn_blocking(build_unified_system_dump)
            .await
            .unwrap_or_else(|e| format!("# OMENSpace Full System Dump\n\nError collecting report: {}", e))
    }

    #[zbus(signal)]
    pub async fn telemetry_updated(ctxt: &zbus::SignalContext<'_>, json_stats: &str) -> zbus::Result<()>;
}

fn build_unified_system_dump() -> String {
    let specs = get_hardware_specs();
    let stats = fetch_system_stats();
    let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
        .unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();
    let manufacturer = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor")
        .unwrap_or_else(|_| "HP".to_string()).trim().to_string();
    let product_name = std::fs::read_to_string("/sys/class/dmi/id/product_name")
        .unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();
    let is_verified = crate::capabilities::LinuxCapabilityClassifier::is_board_verified(&board_id);
    let init_sys = if std::path::Path::new("/run/systemd/system").exists() {
        "systemd"
    } else if std::path::Path::new("/run/openrc").exists() {
        "OpenRC"
    } else {
        "Non-systemd"
    };
    let mut secure_boot = "Unknown".to_string();
    if let Ok(mut entries) = glob::glob("/sys/firmware/efi/efivars/SecureBoot-*") {
        if let Some(Ok(p)) = entries.next() {
            if let Ok(b) = std::fs::read(&p) {
                secure_boot = if *b.last().unwrap_or(&0) == 1 { "Enabled".to_string() } else { "Disabled".to_string() };
            }
        }
    }

    let mut report = String::new();
    report.push_str(&format!("# OMENSpace Full System Dump — {} ({})\n\n", product_name, board_id));
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
    report.push_str(&format!("| Init System       | `{}` |\n", init_sys));
    report.push_str(&format!("| Secure Boot       | `{}` |\n", secure_boot));

    // ── Hardware Probe ───────────────────────────────────────
    report.push_str("\n## Hardware Probe\n\n| Field | Value |\n|-------|-------|\n");
    report.push_str(&format!("| Manufacturer   | `{}` |\n", manufacturer));
    report.push_str(&format!("| Product Name   | `{}` |\n", product_name));
    report.push_str(&format!("| Board ID       | `{}` ({}) |\n", board_id, if is_verified { "Verified" } else { "Unverified" }));
    report.push_str(&format!("| BIOS Version   | `{}` |\n", specs.bios_version));
    report.push_str(&format!("| CPU            | `{}` |\n", specs.cpu_spec));
    report.push_str(&format!("| GPU            | `{}` |\n", specs.gpu_spec));
    report.push_str(&format!("| RAM            | `{}` |\n", specs.ram_spec));
    report.push_str(&format!("| NVIDIA Driver  | `{}` |\n", specs.nvidia_driver));

    // ── Kernel Modules & Conflicts ───────────────────────────
    let proc_modules = std::fs::read_to_string("/proc/modules").unwrap_or_default();
    let has_hp_wmi = proc_modules.contains("hp_wmi");
    let has_hp_omen_extra = proc_modules.contains("hp_omen_extra");
    let ec_io_exists = std::path::Path::new("/sys/kernel/debug/ec/ec0/io").exists();
    let has_ec_sys = proc_modules.contains("ec_sys") || ec_io_exists;
    let conflicts = crate::conflict_detector::ConflictDetector::check_conflicts();

    report.push_str("\n## Driver & Service Status\n\n");
    report.push_str(&format!(
        "- Kernel Modules: `hp_wmi={}` | `hp_omen_extra={}` | `ec_sys={}`\n",
        if has_hp_wmi { "loaded" } else { "missing" },
        if has_hp_omen_extra { "loaded" } else { "not loaded" },
        if has_ec_sys { "active" } else { "not loaded (optional)" },
    ));
    if let Ok(out) = std::process::Command::new("modinfo").args(["-n", "hp-wmi"]).output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            report.push_str(&format!("- `hp-wmi` module path: `{}`\n", p));
        }
    }
    if conflicts.has_conflicts {
        report.push_str(&format!("- Conflicts: {}\n", conflicts.conflicting_processes.join("; ")));
    } else {
        report.push_str("- Conflicts: None detected\n");
    }

    // ── Live Fan & Sensor Telemetry ──────────────────────────
    let mut hp_fan1_exists = false;
    let mut hp_fan2_exists = false;
    let mut hwmon_rows = Vec::new();
    if let Ok(entries) = glob::glob("/sys/class/hwmon/hwmon*") {
        for entry in entries.filter_map(Result::ok) {
            let name = std::fs::read_to_string(entry.join("name"))
                .unwrap_or_else(|_| "?".to_string());
            let name = name.trim();
            let fan1_p = entry.join("fan1_input");
            let fan2_p = entry.join("fan2_input");
            if matches!(name, "hp" | "hp_wmi" | "hp-omen" | "acpi_fan") {
                if fan1_p.exists() { hp_fan1_exists = true; }
                if fan2_p.exists() { hp_fan2_exists = true; }
            }
            let fan1 = std::fs::read_to_string(&fan1_p).map(|v| v.trim().to_string()).unwrap_or_else(|_| "—".to_string());
            let fan2 = std::fs::read_to_string(&fan2_p).map(|v| v.trim().to_string()).unwrap_or_else(|_| "—".to_string());
            let fan1_max = std::fs::read_to_string(entry.join("fan1_max")).map(|v| v.trim().to_string()).unwrap_or_else(|_| "—".to_string());
            let pwm1 = std::fs::read_to_string(entry.join("pwm1")).map(|v| v.trim().to_string()).unwrap_or_else(|_| "—".to_string());
            let pwm1_en = std::fs::read_to_string(entry.join("pwm1_enable")).map(|v| v.trim().to_string()).unwrap_or_else(|_| "—".to_string());
            let hwmon_name = entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if fan1 != "—" || fan2 != "—" || pwm1_en != "—" || pwm1 != "—" {
                hwmon_rows.push(format!("| `{}` | `{}` | {} | {} | {} | {} | {} |", hwmon_name, name, fan1, fan2, fan1_max, pwm1, pwm1_en));
            }
        }
    }

    report.push_str("\n## Live Telemetry & Fans\n\n");
    report.push_str(&format!(
        "- CPU Fan (fan1): {} (CPU Temp: {} °C)\n",
        if hp_fan1_exists || stats.fan1_rpm > 0 {
            format!("{} RPM", stats.fan1_rpm)
        } else {
            "not exposed by hwmon".to_string()
        },
        stats.cpu_temp
    ));
    // Fix #359: distinguish fan2_input present at 0 RPM (dGPU suspended) from truly absent
    report.push_str(&format!(
        "- GPU Fan (fan2): {} (GPU Temp: {} °C, State: {})\n",
        if stats.fan2_rpm > 0 {
            format!("{} RPM", stats.fan2_rpm)
        } else if hp_fan2_exists {
            "0 RPM (idle / dGPU suspended)".to_string()
        } else {
            "not exposed by hwmon".to_string()
        },
        stats.gpu_temp,
        stats.gpu_sample_state.label()
    ));
    report.push_str(&format!(
        "- Power: CPU {:.1} W | GPU {:.1} W | Total {:.1} W\n",
        stats.cpu_pwr, stats.gpu_pwr, stats.total_pwr
    ));
    let pp = std::fs::read_to_string("/sys/firmware/acpi/platform_profile")
        .or_else(|_| std::fs::read_to_string("/sys/devices/platform/hp-wmi/platform_profile"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unavailable".to_string());
    let pp_choices = std::fs::read_to_string("/sys/firmware/acpi/platform_profile_choices")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "—".to_string());
    report.push_str(&format!("- Platform Profile: `{}` (choices: `{}`)\n", pp, pp_choices));

    // ── Battery Telemetry ────────────────────────────────────
    if let Some(ref bat) = stats.battery {
        report.push_str("\n## Battery Telemetry\n\n| Field | Value |\n|-------|-------|\n");
        report.push_str(&format!("| Battery Node | `{}` ({:?}, AC Online: `{}`) |\n", bat.name, bat.status, bat.ac_online));
        report.push_str(&format!("| Charge / Health | `{}%` / `{}` |\n",
            bat.charge_pct,
            bat.health_pct.map(|h| format!("{:.1}%", h)).unwrap_or_else(|| "—".to_string())
        ));
        if let (Some(now), Some(full)) = (bat.energy_now_wh, bat.energy_full_wh) {
            let des_str = bat.energy_design_wh.map(|d| format!(" / {:.2} Wh design", d)).unwrap_or_default();
            report.push_str(&format!("| Energy | `{:.2} / {:.2} Wh{}` |\n", now, full, des_str));
        }
        if let Some(cycles) = bat.cycle_count {
            report.push_str(&format!("| Cycle Count | `{}` |\n", cycles));
        }
    }

    // ── hwmon Paths ──────────────────────────────────────────
    report.push_str("\n## hwmon Sensor Paths\n\n");
    report.push_str("| hwmon | Driver | fan1_input | fan2_input | fan1_max | pwm1 | pwm1_enable |\n");
    report.push_str("|-------|--------|------------|------------|----------|------|-------------|\n");
    if hwmon_rows.is_empty() {
        report.push_str("| — | (no fan hwmon nodes found) | — | — | — | — | — |\n");
    } else {
        for r in hwmon_rows {
            report.push_str(&r);
            report.push('\n');
        }
    }

    // ── GPU MUX & Sysfs Nodes ────────────────────────────────
    report.push_str("\n## GPU MUX & Platform Sysfs\n\n");
    for node in [
        "/sys/devices/platform/hp-wmi/gpu_mux_mode",
        "/sys/devices/platform/hp-wmi/graphics_mode",
        "/sys/devices/platform/hp-wmi/gpu_tgp",
        "/sys/devices/platform/hp-wmi/gpu_ppab",
        "/sys/devices/platform/hp-wmi/thermal_profile",
        "/sys/devices/platform/hp-omen-extra/all_zones",
        "/sys/devices/platform/hp-omen-extra/lightbar",
    ] {
        let state = if std::path::Path::new(node).exists() {
            std::fs::read_to_string(node)
                .map(|v| format!("exists (`{}`)", v.trim()))
                .unwrap_or_else(|_| "exists (unreadable)".to_string())
        } else {
            "missing".to_string()
        };
        report.push_str(&format!("- `{}`: {}\n", node, state));
    }

    // ── USB HID & RGB Discovery ──────────────────────────────
    report.push_str("\n## Keyboard RGB & HID Devices\n\n");
    if let Ok(out) = std::process::Command::new("lsusb").output() {
        let out_str = String::from_utf8_lossy(&out.stdout);
        let matches: Vec<&str> = out_str
            .lines()
            .filter(|l| {
                let lo = l.to_lowercase();
                lo.contains("hp") || lo.contains("hewlett") || lo.contains("omen")
                    || lo.contains("03f0") || lo.contains("0d62") || lo.contains("darfon")
                    || lo.contains("0461") || lo.contains("primax") || lo.contains("1038")
                    || lo.contains("keyboard")
            })
            .collect();
        if !matches.is_empty() {
            report.push_str("```\n");
            for l in matches {
                report.push_str(l);
                report.push('\n');
            }
            report.push_str("```\n");
        }
    }
    if let Ok(entries) = glob::glob("/sys/class/hidraw/hidraw*") {
        let mut hid_lines = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let dev_name = entry.file_name().unwrap_or_default().to_string_lossy().to_string();
            let uevent = std::fs::read_to_string(entry.join("device/uevent")).unwrap_or_default();
            let hid_id = uevent.lines().find(|l| l.starts_with("HID_ID=")).unwrap_or("");
            let hid_name = uevent.lines().find(|l| l.starts_with("HID_NAME=")).unwrap_or("");
            let desc = std::fs::read(entry.join("device/report_descriptor")).unwrap_or_default();
            let tag = if desc.starts_with(&[0x05, 0x59, 0x09, 0x01]) {
                " [LampArray 0x59]"
            } else if desc.windows(3).any(|w| w[0] == 0x06 && w[2] == 0xFF) {
                " [Vendor 0xFF]"
            } else {
                ""
            };
            if !hid_id.is_empty() {
                hid_lines.push(format!("- `/dev/{}`: `{}` `{}`{}", dev_name, hid_id.trim_start_matches("HID_ID="), hid_name.trim_start_matches("HID_NAME="), tag));
            }
        }
        for l in hid_lines {
            report.push_str(&l);
            report.push('\n');
        }
    }

    // ── ACPI DSDT/SSDT Summary ───────────────────────────────
    let acpi = crate::acpi_diagnostics::AcpiDiagnosticRunner::analyze_acpi_tables();
    report.push_str("\n## ACPI WMI GUIDs & Methods\n\n");
    report.push_str(&format!("- DSDT: `{}` | SSDT count: `{}`\n", if acpi.dsdt_found { "present" } else { "missing" }, acpi.ssdt_count));
    let active_guids: Vec<String> = acpi.guids.iter().filter(|g| g.detected).map(|g| format!("`{}`", g.guid)).collect();
    let active_methods: Vec<String> = acpi.methods.iter().filter(|m| m.detected).map(|m| format!("`{}`", m.name)).collect();
    report.push_str(&format!("- Active GUIDs: {}\n", if active_guids.is_empty() { "none".to_string() } else { active_guids.join(", ") }));
    report.push_str(&format!("- Active Methods: {}\n", if active_methods.is_empty() { "none".to_string() } else { active_methods.join(", ") }));

    // ── EC Registers Snapshot ────────────────────────────────
    report.push_str("\n## EC Registers — Snapshot\n\n");
    if !ec_io_exists {
        let _ = std::process::Command::new("modprobe").args(["ec_sys", "write_support=1"]).output();
    }
    if let Ok(ec_data) = std::fs::read("/sys/kernel/debug/ec/ec0/io") {
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
        report.push_str("> EC debugfs (`/sys/kernel/debug/ec/ec0/io`) unavailable (optional when `hp-wmi` hwmon is active; kernel may lack `ec_sys`).\n");
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

    // ── Recent Daemon Logs ──────────────────────────────────
    report.push_str("\n## Daemon Log Excerpt (last 20 lines)\n\n```\n");
    let daemon_logs = if std::path::Path::new("/run/systemd/system").exists() {
        std::process::Command::new("journalctl")
            .args(["-u", "omen-space-daemon", "-n", "20", "--no-pager"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    } else {
        std::fs::read_to_string("/var/log/omen-space-daemon.log")
            .map(|s| s.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    };
    if daemon_logs.trim().is_empty() {
        report.push_str("(no daemon logs captured)\n");
    } else {
        report.push_str(&daemon_logs);
        if !daemon_logs.ends_with('\n') {
            report.push('\n');
        }
    }
    report.push_str("```\n");

    report
}
