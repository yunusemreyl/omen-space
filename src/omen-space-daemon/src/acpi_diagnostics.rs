use glob::glob;
use log::info;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AcpiGuidMapping {
    pub guid: String,
    pub name: String,
    pub description: String,
    pub detected: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AcpiMethodMapping {
    pub name: String,
    pub purpose: String,
    pub hardware_feature: String,
    pub detected: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AcpiReport {
    pub dsdt_found: bool,
    pub ssdt_count: usize,
    pub iasl_available: bool,
    pub acpidump_available: bool,
    pub guids: Vec<AcpiGuidMapping>,
    pub methods: Vec<AcpiMethodMapping>,
    pub markdown_summary: String,
}

pub struct AcpiDiagnosticRunner;

impl AcpiDiagnosticRunner {
    pub fn analyze_acpi_tables() -> AcpiReport {
        info!("Extracting and analyzing ACPI DSDT/SSDT tables for HP WMI & EC symbols...");

        let dsdt_path = Path::new("/sys/firmware/acpi/tables/DSDT");
        let dsdt_found = dsdt_path.exists();

        let ssdt_entries: Vec<PathBuf> = glob("/sys/firmware/acpi/tables/SSDT*")
            .ok()
            .map(|g| g.filter_map(Result::ok).collect())
            .unwrap_or_default();
        let ssdt_count = ssdt_entries.len();

        let iasl_available = Command::new("iasl").arg("-v").output().is_ok();
        let acpidump_available = Command::new("acpidump").arg("-h").output().is_ok();

        // Target HP WMI GUIDs
        let mut guids = vec![
            AcpiGuidMapping {
                guid: "5FB70034-AE3E-411C-8E97-10B9B93B54D0".to_string(),
                name: "HP WMI Thermal & Power Profile GUID".to_string(),
                description:
                    "Controls Performance Modes (Quiet, Balanced, Performance/OMEN) and Fan Curves"
                        .to_string(),
                detected: false,
            },
            AcpiGuidMapping {
                guid: "95F24279-4D7B-4334-9387-AC7F57838F64".to_string(),
                name: "HP WMI MUX Switch GUID".to_string(),
                description: "Controls GPU Switching (Hybrid, Discrete, Integrated graphics mode)"
                    .to_string(),
                detected: false,
            },
            AcpiGuidMapping {
                guid: "08D86410-C7D0-474F-94F0-A0E18A49C4C4".to_string(),
                name: "HP WMI System Telemetry & Event GUID".to_string(),
                description: "Handles OMEN physical hotkeys, keybindings, and sensor notifications"
                    .to_string(),
                detected: false,
            },
            AcpiGuidMapping {
                guid: "2D141180-1034-43B0-A23A-31AE4484B429".to_string(),
                name: "HP OMEN Lighting Control GUID".to_string(),
                description: "Handles 4-zone and per-key keyboard RGB backlight control"
                    .to_string(),
                detected: false,
            },
        ];

        // Target ACPI WMI & EC Methods
        let mut methods = vec![
            AcpiMethodMapping {
                name: "WMAA".to_string(),
                purpose: "HP WMI Command Method A".to_string(),
                hardware_feature: "Thermal Policies, Performance Profile Switching & Power Limits"
                    .to_string(),
                detected: false,
            },
            AcpiMethodMapping {
                name: "WMBB".to_string(),
                purpose: "HP WMI Command Method B".to_string(),
                hardware_feature: "Keyboard RGB Lighting & Zone Colors".to_string(),
                detected: false,
            },
            AcpiMethodMapping {
                name: "WMCB".to_string(),
                purpose: "HP WMI Command Method C".to_string(),
                hardware_feature: "Direct Fan Speed Queries & Diagnostic Sensor Readouts"
                    .to_string(),
                detected: false,
            },
            AcpiMethodMapping {
                name: "HWMC".to_string(),
                purpose: "HP WMI Master Controller".to_string(),
                hardware_feature: "BIOS ACPI WMI dispatcher".to_string(),
                detected: false,
            },
            AcpiMethodMapping {
                name: "HPCC".to_string(),
                purpose: "HP Command Center ACPI Node".to_string(),
                hardware_feature: "OMEN Gaming Hub hardware handoff".to_string(),
                detected: false,
            },
        ];

        // Collect all ACPI table paths to scan individually without heavy buffer allocation
        let mut table_paths = Vec::new();
        if dsdt_found {
            table_paths.push(dsdt_path.to_path_buf());
        }
        table_paths.extend(ssdt_entries.iter().cloned());

        for table_path in &table_paths {
            if let Ok(bytes) = fs::read(table_path) {
                for g in &mut guids {
                    if !g.detected {
                        let guid_bytes = g.guid.as_bytes();
                        let clean_guid_bytes = g.guid.replace('-', "").into_bytes();
                        if bytes
                            .windows(guid_bytes.len())
                            .any(|w| w.eq_ignore_ascii_case(guid_bytes))
                            || bytes
                                .windows(clean_guid_bytes.len())
                                .any(|w| w.eq_ignore_ascii_case(&clean_guid_bytes))
                        {
                            g.detected = true;
                        }
                    }
                }

                for m in &mut methods {
                    if !m.detected {
                        let m_bytes = m.name.as_bytes();
                        if bytes.windows(m_bytes.len()).any(|w| w == m_bytes) {
                            m.detected = true;
                        }
                    }
                }
            }
        }

        // Check HID Per-Key RGB device availability across HP, Darfon, Primax, SteelSeries, and HID LampArray (#364)
        let mut hid_device_info =
            "No HP/ODM HID Per-Key RGB Controller detected on /dev/hidraw*".to_string();
        if let Ok(entries) = glob("/sys/class/hidraw/hidraw*") {
            let mut found_nodes = Vec::new();
            for entry in entries.filter_map(Result::ok) {
                let uevent_path = entry.join("device/uevent");
                let dev_name = entry
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let uevent = fs::read_to_string(&uevent_path).unwrap_or_default();
                let desc = fs::read(entry.join("device/report_descriptor")).unwrap_or_default();
                let is_lamparray = desc.starts_with(&[0x05, 0x59, 0x09, 0x01]);
                let is_known_vid = uevent.contains("03F0")
                    || uevent.contains("0D62")
                    || uevent.contains("0461")
                    || uevent.contains("1038");
                if is_known_vid || is_lamparray {
                    let hid_id = uevent
                        .lines()
                        .find(|l| l.starts_with("HID_ID="))
                        .unwrap_or("");
                    let hid_name = uevent
                        .lines()
                        .find(|l| l.starts_with("HID_NAME="))
                        .unwrap_or("");
                    found_nodes.push(format!(
                        "/dev/{} ({} {}{})",
                        dev_name,
                        hid_id.trim_start_matches("HID_ID="),
                        hid_name.trim_start_matches("HID_NAME="),
                        if is_lamparray { ", LampArray" } else { "" }
                    ));
                }
            }
            if !found_nodes.is_empty() {
                hid_device_info = format!("Active: {}", found_nodes.join("; "));
            }
        }

        // Generate Markdown analysis table
        let mut md_lines = vec![
            "# ACPI DSDT & SSDT Symbol Analysis Report".to_string(),
            format!(
                "- **DSDT Table Present:** {}",
                if dsdt_found {
                    "Yes (/sys/firmware/acpi/tables/DSDT)"
                } else {
                    "No"
                }
            ),
            format!("- **SSDT Tables Found:** {}", ssdt_count),
            format!("- **HID Per-Key RGB Controller:** {}", hid_device_info),
            format!(
                "- **ACPICA `iasl` Available:** {}",
                if iasl_available {
                    "Yes"
                } else {
                    "No (Install acpica-tools for full disassembly)"
                }
            ),
            format!(
                "- **ACPICA `acpidump` Available:** {}",
                if acpidump_available { "Yes" } else { "No" }
            ),
            String::new(),
            "## Discovered HP WMI GUIDs & ACPI Methods".to_string(),
            "| ACPI Symbol / GUID | Name & Purpose | Hardware Feature Mapped | Detection Status |"
                .to_string(),
            "| --- | --- | --- | --- |".to_string(),
        ];

        for g in &guids {
            md_lines.push(format!(
                "| `{}` | {} | {} | {} |",
                g.guid,
                g.name,
                g.description,
                if g.detected {
                    "Confirmed Active"
                } else {
                    "Not Present"
                }
            ));
        }

        for m in &methods {
            md_lines.push(format!(
                "| Method `{}` | {} | {} | {} |",
                m.name,
                m.purpose,
                m.hardware_feature,
                if m.detected {
                    "Confirmed Active"
                } else {
                    "Not Present"
                }
            ));
        }

        let markdown_summary = md_lines.join("\n");

        AcpiReport {
            dsdt_found,
            ssdt_count,
            iasl_available,
            acpidump_available,
            guids,
            methods,
            markdown_summary,
        }
    }

    pub fn generate_triage_bundle() -> String {
        let timestamp = chrono_like_timestamp();
        let bundle_dir = format!("/tmp/omen-triage-{}", timestamp);
        let archive_path = format!("/tmp/omen-space-triage-{}.tar.gz", timestamp);

        let _ = fs::create_dir_all(&bundle_dir);

        // 1. Run ACPI analysis
        let acpi_report = Self::analyze_acpi_tables();
        let _ = fs::write(
            format!("{}/acpi-dsdt-ssdt-mapping.md", bundle_dir),
            &acpi_report.markdown_summary,
        );

        // 2. Run WMI 1000-point diagnostics
        let wmi_report = crate::wmi_diagnostics::WmiDiagnosticRunner::run_full_suite();
        if let Ok(json) = serde_json::to_string_pretty(&wmi_report) {
            let _ = fs::write(format!("{}/wmi-1000-diagnostics.json", bundle_dir), json);
        }

        // 3. System info
        let board_id = read_dmi_field("board_name");
        let product_name = read_dmi_field("product_name");
        let bios_ver = read_dmi_field("bios_version");
        let sys_info = serde_json::json!({
            "board_id": board_id,
            "product_name": product_name,
            "bios_version": bios_ver,
            "kernel": read_file_line("/proc/sys/kernel/osrelease"),
            "os_release": read_file_line("/etc/os-release"),
        });
        let _ = fs::write(
            format!("{}/system-info.json", bundle_dir),
            serde_json::to_string_pretty(&sys_info).unwrap_or_default(),
        );

        // 4. Extract journal / dmesg for hp-wmi & omen
        let dmesg_output = Command::new("sh")
            .arg("-c")
            .arg("dmesg | grep -Ei 'hp-wmi|omen|acpi|wmi' | tail -n 250")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_else(|_| "Failed to collect dmesg".to_string());
        let _ = fs::write(format!("{}/journal-hp-wmi.log", bundle_dir), dmesg_output);

        // 5. Generate GitHub Issue Markdown Template
        let issue_template = format!(
            "# OMENSpace Triage Report - {} ({})\n\n\
            ## System Details\n\
            - **Product Name:** {}\n\
            - **Board ID:** {}\n\
            - **BIOS Version:** {}\n\
            - **Kernel:** {}\n\n\
            ## Diagnostics Status\n\
            - **WMI & EC Score:** {}\n\
            - **ACPI DSDT Found:** {}\n\
            - **SSDT Tables:** {}\n\n\
            ## ACPI Symbol Mapping\n\
            ```markdown\n\
            {}\n\
            ```\n\n\
            *Generated automatically by OMENSpace Daemon on {}*",
            product_name,
            board_id,
            product_name,
            board_id,
            bios_ver,
            read_file_line("/proc/sys/kernel/osrelease"),
            wmi_report.status_summary,
            if acpi_report.dsdt_found { "Yes" } else { "No" },
            acpi_report.ssdt_count,
            acpi_report.markdown_summary,
            timestamp
        );
        let _ = fs::write(
            format!("{}/github-issue-template.md", bundle_dir),
            issue_template,
        );

        // 6. Compress into .tar.gz
        let tar_cmd = Command::new("tar")
            .args([
                "-czf",
                &archive_path,
                "-C",
                "/tmp",
                &format!("omen-triage-{}", timestamp),
            ])
            .output();

        if tar_cmd.is_ok() && Path::new(&archive_path).exists() {
            info!("Successfully generated triage bundle at {}", archive_path);
            archive_path
        } else {
            bundle_dir
        }
    }
}

fn read_dmi_field(entry: &str) -> String {
    let path = format!("/sys/class/dmi/id/{}", entry);
    fs::read_to_string(path)
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string()
}

fn read_file_line(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|_| "Unknown".to_string())
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
