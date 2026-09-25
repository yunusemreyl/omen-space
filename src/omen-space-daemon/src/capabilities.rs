#![allow(dead_code)]
#![allow(unused_imports)]
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum LinuxCapabilityClass {
    FullControl,
    ProfileOnly,
    TelemetryOnly,
    UnsupportedControl,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinuxCapabilityAssessment {
    pub capability_class: LinuxCapabilityClass,
    pub supports_manual_fan_control: bool,
    pub supports_profile_control: bool,
    pub supports_telemetry: bool,
    pub reason: String,
}

impl LinuxCapabilityAssessment {
    pub fn capability_key(&self) -> &str {
        match self.capability_class {
            LinuxCapabilityClass::FullControl => "full-control",
            LinuxCapabilityClass::ProfileOnly => "profile-only",
            LinuxCapabilityClass::TelemetryOnly => "telemetry-only",
            LinuxCapabilityClass::UnsupportedControl => "unsupported-control",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelCapabilities {
    pub product_id: String,
    pub model_name: String,
    pub model_year: u32,
    pub family: String,
    
    // Fan Control
    pub supports_fan_control_wmi: bool,
    pub supports_fan_control_ec: bool,
    pub supports_fan_curves: bool,
    pub supports_independent_fan_curves: bool,
    pub supports_rpm_readback: bool,
    pub fan_zone_count: u32,
    pub max_fan_speed_percent: u32,
    pub min_fan_speed_percent: u32,
    
    // Performance Modes
    pub supports_performance_modes: bool,
    pub performance_modes: Vec<String>,
    pub allow_decoupled_wmi_thermal_policy_fallback: bool,
    
    // GPU
    pub has_mux_switch: bool,
    pub supports_gpu_power_boost: bool,
    pub supports_advanced_optimus: bool,
    
    // Lighting
    pub has_keyboard_backlight: bool,
    pub has_four_zone_rgb: bool,
    pub has_per_key_rgb: bool,
    pub has_light_bar: bool,
    
    // Power / Undervolt
    pub supports_undervolt: bool,
    pub supports_tcc_offset: bool,
    pub supports_power_limits: bool,
    pub supports_battery_care: bool,
    
    // Low-level OS Assessment
    pub linux_assessment: Option<LinuxCapabilityAssessment>,
    
    pub notes: String,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            product_id: "DEFAULT".to_string(),
            model_name: "Unknown HP System".to_string(),
            model_year: 2023,
            family: "HP".to_string(),
            
            supports_fan_control_wmi: true,
            supports_fan_control_ec: false,
            supports_fan_curves: true,
            supports_independent_fan_curves: true,
            supports_rpm_readback: true,
            fan_zone_count: 2,
            max_fan_speed_percent: 100,
            min_fan_speed_percent: 0,
            
            supports_performance_modes: true,
            performance_modes: vec!["Default".to_string(), "Performance".to_string(), "Cool".to_string()],
            allow_decoupled_wmi_thermal_policy_fallback: false,
            
            has_mux_switch: false,
            supports_gpu_power_boost: true,
            supports_advanced_optimus: false,
            
            has_keyboard_backlight: true,
            has_four_zone_rgb: true,
            has_per_key_rgb: false,
            has_light_bar: false,
            
            supports_undervolt: true,
            supports_tcc_offset: true,
            supports_power_limits: true,
            supports_battery_care: true,
            linux_assessment: None,
            
            notes: "".to_string(),
        }
    }
}

pub struct LinuxCapabilityClassifier;

impl LinuxCapabilityClassifier {
    pub fn assess(is_root: bool, board_id: &str, _model: &str) -> LinuxCapabilityAssessment {
        let is_wmaa_abort_prone = Self::is_wmaa_abort_prone_board(board_id);
        
        let has_ec_access = Path::new("/sys/kernel/debug/ec/ec0/io").exists();
        let has_hp_wmi_path = Path::new("/sys/devices/platform/hp-wmi").exists();
        // Check both underscore and hyphen naming (kernel version dependent)
        let has_thermal_profile =
            Path::new("/sys/devices/platform/hp-wmi/thermal_profile").exists() ||
            Path::new("/sys/devices/platform/hp-wmi/thermal-profile").exists();
        let has_platform_profile =
            Path::new("/sys/devices/platform/hp-wmi/platform_profile").exists() ||
            Path::new("/sys/devices/platform/hp-wmi/platform-profile").exists();
        let has_acpi_platform_profile = Path::new("/sys/firmware/acpi/platform_profile").exists();
        let has_fan1_output = Path::new("/sys/devices/platform/hp-wmi/fan1_output").exists();
        let has_fan2_output = Path::new("/sys/devices/platform/hp-wmi/fan2_output").exists();
        
        // Basic hwmon checks
        let mut has_hwmon_fan_access = false;
        if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
            for entry in entries.filter_map(Result::ok) {
                if entry.path().join("pwm1_enable").exists() {
                    has_hwmon_fan_access = true;
                }
            }
        }
        
        let has_manual_fan_control = has_ec_access || has_fan1_output || has_fan2_output;
        let has_profile_control = has_thermal_profile || has_platform_profile || has_acpi_platform_profile || has_hwmon_fan_access;
        let has_telemetry = has_hp_wmi_path || has_manual_fan_control || has_profile_control;
        
        if is_wmaa_abort_prone && has_manual_fan_control {
            let mut reason = "Board 8BCD has field reports of ACPI WMAA/WHCM aborts where WMI-backed fan, RGB, and battery paths can report success without hardware effect. Treat visible manual/profile fan paths as degraded until an effective write/readback check proves control.".to_string();
            if !is_root {
                reason.push_str(" Run with sudo for write/readback validation.");
            }
            return LinuxCapabilityAssessment {
                capability_class: if has_profile_control { LinuxCapabilityClass::ProfileOnly } else { LinuxCapabilityClass::TelemetryOnly },
                supports_manual_fan_control: false,
                supports_profile_control: has_profile_control,
                supports_telemetry: true,
                reason,
            };
        }

        if has_manual_fan_control {
            let mut reason = if has_hwmon_fan_access {
                "Manual fan control is available through hwmon pwm/fan targets."
            } else if has_fan1_output {
                "Manual fan control is available through hp-wmi fan output files."
            } else {
                "Manual fan control is available through legacy EC access."
            }.to_string();
            
            if !is_root {
                reason.push_str(" Run with sudo to use write-capable controls.");
            }
            
            return LinuxCapabilityAssessment {
                capability_class: LinuxCapabilityClass::FullControl,
                supports_manual_fan_control: true,
                supports_profile_control: has_profile_control,
                supports_telemetry: true,
                reason,
            };
        }

        if has_profile_control {
            let mut reason = "Thermal/platform profile control is available, but firmware does not expose manual fan target/output interfaces on this board.".to_string();
            if !is_root {
                reason.push_str(" Run with sudo to apply profile changes.");
            }
            if is_wmaa_abort_prone {
                reason.push_str(" Board 8BCD is currently treated as degraded profile control because field diagnostics show ACPI WMAA/WHCM aborts.");
            }
            
            return LinuxCapabilityAssessment {
                capability_class: LinuxCapabilityClass::ProfileOnly,
                supports_manual_fan_control: false,
                supports_profile_control: true,
                supports_telemetry: true,
                reason,
            };
        }

        if has_telemetry {
            return LinuxCapabilityAssessment {
                capability_class: LinuxCapabilityClass::TelemetryOnly,
                supports_manual_fan_control: false,
                supports_profile_control: false,
                supports_telemetry: true,
                reason: "Telemetry paths are present, but no writable EC, hp-wmi, hwmon target, or platform profile control interface is exposed.".to_string(),
            };
        }

        LinuxCapabilityAssessment {
            capability_class: LinuxCapabilityClass::UnsupportedControl,
            supports_manual_fan_control: false,
            supports_profile_control: false,
            supports_telemetry: false,
            reason: "No supported Linux control interface was detected. This is usually a kernel exposure gap or missing hp-wmi/ec_sys support.".to_string(),
        }
    }

    pub fn is_wmaa_abort_prone_board(board_id: &str) -> bool {
		// 8BCD: ACPI WMAA/WHCM aborts (field-reported)
		// 8C75: broken GETB zero-length CreateField → AE_AML_BUFFER_LIMIT on all WMID methods
		// 878A: AE_AML_BUFFER_LIMIT flood → EC lockups
		// 8A42: AE_AML_BUFFER_LIMIT on WMID WMBA/WMAA + hp_wmi failed to set platform profile 5: -22
		//       (Issue #242 dmesg confirmed; WMI BIOS command path broken, fallback to hp_wmi platform_profile)
		matches!(board_id.trim().to_uppercase().as_str(), "8BCD" | "8C75" | "878A" | "8A42")
	}

    /// Returns true when the board_id appears in the community-verified list.
    ///
    /// A "verified" board is one where a user has run the full checklist
    /// (modes, fans, power gain, GPU power, graphics, lighting) and confirmed
    /// every control behaves as expected.  Boards not in this list still receive
    /// full daemon service; the GUI simply shows a one-time banner asking the
    /// user to file a verification issue.
    pub fn is_board_verified(board_id: &str) -> bool {
        // List mirrors the "Verified" table in docs/ and known-good community
        // reports from GitHub issues / Discord field logs.
        matches!(
            board_id.trim().to_uppercase().as_str(),
            // OMEN Legacy
            "8A14" | "8A15" | "8574" | "8600" | "8787" | "878C" | "88D2" | "8BAD"
            // OMEN 16
            | "8BAF" | "8BB0" | "8BCA" | "8BAB" | "8C76" | "8C77" | "8BA9"
            | "8D24" | "8E35" | "8D26" | "8D2F"
            // OMEN 17
            | "8BB1" | "8A18" | "8E10" | "8603" | "8B9D" | "8B9E"
            // OMEN Transcend
            | "8C3A" | "8C3B" | "8C58" | "8E41"
            // Victus
            | "88D9" | "88DA" | "8A3E" | "8DCD" | "8A26" | "8A25"
            | "8BD4" | "8C2F" | "88DB" | "88EC" | "88EE" | "8C3F"
            | "8E5E" | "8A3D"
            // Victus 16-d (kernel hp-wmi list)
            | "88F8"
            // Victus 16-r/s (kernel hp-wmi list)
            | "8BBE" | "8BD5" | "8C99" | "8C9C"
            // Victus — Issue #245 community-verified
            | "8E5D"
            // OMEN 16 8A42 — Issue #242 confirmed working (fan control degraded, profile OK via fallback)
            | "8A42"
        )
    }
}

pub fn detect(board_id: &str, product_name: &str, cpu_model: &str) -> ModelCapabilities {
    let mut cap = get_known_model(board_id).or_else(|| {
        let prod_lower = product_name.to_lowercase();
        // Fallback by product name if exact board_id is not found
        get_all_models().iter().find(|m| prod_lower.contains(&m.model_name.to_lowercase())).cloned()
    }).unwrap_or_default();
    
    let cpu_upper = cpu_model.to_uppercase();
    let is_hx = cpu_upper.contains("HX");
    let is_amd = cpu_upper.contains("AMD") || cpu_upper.contains("RYZEN");
    
    if cap.family.to_uppercase().contains("VICTUS")
        && !is_hx && !is_amd {
            cap.supports_undervolt = false;
            cap.supports_tcc_offset = false;
            cap.supports_power_limits = false;
        }
    cap
}

macro_rules! model {
    ($id:expr, $name:expr, $year:expr, $family:expr, { $($field:ident : $val:expr),* $(,)? }) => {
        {
            let mut m = ModelCapabilities {
                product_id: $id.to_string(),
                model_name: $name.to_string(),
                model_year: $year,
                family: $family.to_string(),
                ..Default::default()
            };
            $(
                m.$field = $val;
            )*
            m
        }
    };
}

fn get_all_models() -> &'static [ModelCapabilities] {
    static MODELS: std::sync::OnceLock<Vec<ModelCapabilities>> = std::sync::OnceLock::new();
    MODELS.get_or_init(|| vec![
        model!("8A14", "OMEN 15 (2020) Intel", 2020, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: true, supports_fan_curves: true, supports_independent_fan_curves: true, has_mux_switch: false, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "Well-tested model with full WMI BIOS support".to_string() }),
        model!("8A15", "OMEN 15 (2020) AMD", 2020, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: true, supports_fan_curves: true, supports_undervolt: false, has_four_zone_rgb: true }),
        model!("8574", "OMEN 15-dc1xxx (2019) Intel", 2019, "Legacy", { supports_fan_control_wmi: false, supports_fan_control_ec: true, supports_fan_curves: true, supports_independent_fan_curves: true, supports_rpm_readback: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: false, supports_gpu_power_boost: true, has_keyboard_backlight: true, has_four_zone_rgb: false, supports_undervolt: true, supports_tcc_offset: false, supports_power_limits: false, notes: "Discord field report - OMEN 15-dc1077tx (ProductId 8574): WMI BIOS command path not functional, EC fan control and PawnIO undervolt runtime available; RGB kept conservative until exact keyboard protocol is verified.".to_string() }),
        model!("8600", "OMEN 15-dh0xxx (2019) Intel", 2019, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: false, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: false, supports_gpu_power_boost: true, has_keyboard_backlight: true, has_four_zone_rgb: false, supports_undervolt: true, supports_power_limits: false, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "Discord wafflist 2026-06-15 - OMEN by HP Laptop 15-dh0xxx / ProductId 8600. Exact conservative legacy profile added after FAMILY_LEGACY fallback, barely-effective fan modes except Max, missing PawnIO, CPU temp stuck near 28C, CPU power 0W, and fan RPM 0. Direct EC writes and RPM readback remain disabled until PawnIO/readback validation confirms the board path; WMI thermal-policy fallback enabled for Quick Profiles.".to_string() }),
        model!("8787", "OMEN 15-en0038ur (2020) AMD", 2020, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: false, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, notes: "GitHub #120 - HP OMEN Laptop 15-en0038ur, ProductId 8787. Initial support from diagnostics; fan RPM readback remains pending verification.".to_string() }),
        model!("878C", "OMEN 15-ek0xxx (2020) Intel", 2020, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: false, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: true, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "Discord Sky 2026-06-12 - OMEN Laptop 15-ek0xxx / ProductId 878C, i7-10750H + GTX 1650 Ti. Exact conservative legacy WMI profile added after Performance/Balanced/Quiet left fans near low RPM at 99C while Custom Max worked; direct EC writes disabled and WMI thermal-policy fallback enabled pending PL1/PL2 readback validation.".to_string() }),
        model!("88D2", "OMEN by HP Laptop 15z-en100 (2021) AMD", 2021, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: true, supports_undervolt: false, notes: "GitHub #132/#146 - ProductId 88D2 / 15z-en100. Conservative legacy WMI V1 profile; direct EC writes disabled and independent curves held off pending field verification. #146 separately reports fans stuck at 100% until an OmenCore restart clears it - not a capability-flag issue, needs a diagnostics export captured during the actual stuck state to trace further.".to_string() }),
        model!("8BAD", "OMEN 15 (2021) Intel", 2021, "Legacy", { supports_fan_control_wmi: true, supports_fan_curves: true, has_four_zone_rgb: true }),
        model!("8BAF", "OMEN 16 (2021) Intel", 2021, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true }),
        model!("8BB0", "OMEN 16 (2021) AMD", 2021, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_undervolt: false, has_four_zone_rgb: true }),
        model!("8CD0", "OMEN 16 (2022) Intel", 2022, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, supports_advanced_optimus: true, has_four_zone_rgb: true }),
        model!("8CD1", "OMEN 16 (2022) AMD", 2022, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_undervolt: false, supports_advanced_optimus: true, has_four_zone_rgb: true }),
        model!("8A4C", "OMEN 16 (2022) AMD", 2022, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_undervolt: false, has_four_zone_rgb: true, notes: "GitHub #243".to_string() }),
        model!("8A44", "OMEN 16 (2022) n0xxx AMD", 2022, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, supports_undervolt: false, has_four_zone_rgb: true, notes: "GitHub #112 — OMEN 16-n0xxx. Capabilities inferred from adjacent OMEN 16 generations; needs user verification.".to_string() }),
        model!("8A43", "OMEN 16 (2022) n0xxx AMD", 2022, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, supports_undervolt: false, has_four_zone_rgb: true, notes: "GitHub #121 / Discord 2026-05-25 — Hades 8A43 exact ProductId profile added to avoid model-name-pattern inference. HP serial lookup reports OMEN Gaming Laptop 16-n0002ni / 6G103EA. Fan diagnostics show practical V1 ceiling near level 60 (GPU ~60, CPU ~58), so max fan level override is set to 60 for safer verification/normalization.".to_string() }),
        model!("8A42", "OMEN 16 (2022) n0xxx AMD", 2022, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, supports_undervolt: false, has_four_zone_rgb: true, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "GitHub #216/#242 — OMEN 16-n0xxx (Ryzen 7 6800H + RX 6650 XT), ProductId 8A42, BIOS F.27. Issue #242 dmesg confirms AE_AML_BUFFER_LIMIT on WMID WMBA/WMAA/HWMC and hp_wmi failed to set platform profile 5: -22. The WMI BIOS command path (OMEN native) is non-functional on this board; power profile changes fall back to the hp_wmi/acpi platform_profile sysfs interface. Fan telemetry (EC RPM) confirmed working. Fan mode BIOS commands (WMI V1) may also be affected; EC path preferred. AllowDecoupledWmiThermalPolicyFallback=true ensures profile changes still reach the kernel.".to_string() }),
        model!("8BCA", "OMEN 16 (2023) wf0xxx Intel", 2023, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true }),
        model!("8BCA-AMD", "OMEN 16 (2023) xf0xxx AMD", 2023, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, notes: "GitHub #163 — HP OMEN 16 XF0079AX (Ryzen 7 7840HS + RTX 4070), ProductId 8BCA shared with the wf0xxx Intel SKU. Disambiguated by WMI model name (16-xf0xxx). Capabilities carried over from the wf0xxx sibling (same board/chassis) except SupportsUndervolt=false (AMD, no Intel MSR path); not yet independently confirmed on this specific variant.".to_string() }),
        model!("8BAB", "OMEN 16 (2024) wf1xxx Intel", 2024, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "OMEN 16-wf1xxx (2024 Intel) — Board 8C78. Added for Issue #68. Set UserVerified=true after community confirmation.".to_string() }),
        model!("8C76", "OMEN 16 (2024) wf1xxx Intel", 2024, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "Discord HUrON / HP OMEN 16-WF1015ns 9U8J3EA — ProductId 8C76, i9-14900HX + RTX 4080, BIOS F.19, WMI V1/classic 55-level fan control. Exact entry replaces low-confidence inferred sibling match.".to_string() }),
        model!("8C77", "OMEN 16 (2024) wf1xxx Intel", 2024, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "OMEN 16-wf1xxx (2024 Intel) — ProductId 8C77, BIOS F.19. Crash report 2026-07: FileNotFoundException on Custom Fan Curve/Quiet mode (8BAB V2 mismatch). Profile mirrors confirmed 8C76 sibling; V1 WMI 55-level fan control.".to_string() }),
        model!("8BA9", "OMEN 16-wd0xxx (2023) Intel", 2023, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "Discord GHOST 2026-09-02 — HP OMEN by HP Gaming Laptop 16-wd0xxx, ProductId 8BA9, i7-13620H + RTX 4060. Exact entry replaces low-confidence OMEN16 family fallback with a named, still-conservative identity; flags mirror what the live capability probe already confirmed each session.".to_string() }),
        model!("8B2J", "OMEN 16 (2024) xf0xxx Intel", 2024, "OMEN2024Plus", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "2024 model - may have WMI quirks on older BIOS versions".to_string() }),
        model!("8BCD", "OMEN 16 (2024) xd0xxx AMD", 2024, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "Discord 2026-05-20 + field follow-up 2026-05-29; Discord 2026-06-05/06 - OMEN 16-xd0xxx / ProductId 8BCD (Ryzen + RTX 4050). V1 WMI fan control with practical fan-level ceiling near 63 (~6300 RPM); direct EC and independent curves disabled pending register-layout validation. V1 auto-mode floor clear is enabled to release stale manual fan floors after load/profile handoff.".to_string() }),
        model!("8D24", "OMEN 16 (2025) ap0xxx AMD", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, notes: "2025 AMD model (Ryzen AI 9 365 + RTX 5060). V1 fan control, MaxFanLevel=55. PawnIO requires reboot after first install to activate driver.".to_string() }),
        model!("8E35", "OMEN 16 (2025) ap0xxx AMD", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, notes: "Discord RC1 report - OMEN Gaming Laptop 16-ap0xxx / ProductId 8E35 / SKU 1H85430PWY (Ryzen AI 9 365 + RTX 5060). Same WMI V1 fan profile as 8D24; EC direct remains disabled until validated.".to_string() }),
        model!("8D26", "OMEN 16 (2025) ap0xxx AMD", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, notes: "GitHub #188 - OMEN Gaming Laptop 16-ap0xxx / ProductId 8D26 / SKU 5CD5399MYY (Ryzen AI 7 350 + Radeon 860M iGPU + RTX 5070). Same WMI V1 fan profile as 8D24; reporter confirms hardware already works correctly under that fallback. EC direct remains disabled until validated.".to_string() }),
        model!("8D2F", "OMEN 16-am0xxx (8D2F)", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "GitHub #111 / Discord 2026-05-20 and 2026-05-21; Discord 2026-06-02 follow-up - OMEN Gaming Laptop 16-am0xxx, ProductId 8D2F. Exact board identity confirmed; product ID has appeared across AMD and Intel Core Ultra variants, so direct EC fan writes and independent curves remain disabled. WMI V1 fan/profile control is retained, WMI thermal-policy fallback is enabled for performance modes when EC/MSR power-limit writes are unavailable, and V1 auto-mode floor clear is enabled to let fans ramp down after load.".to_string() }),
        model!("am0xxx_intel_2025_unverified", "OMEN 16 (2025) am0xxx Intel Core Ultra", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: false, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "GitHub #124 - OMEN Gaming Laptop 16-am0168ng / 16-am0xxx (Intel Core Ultra 7-255H + RTX 5070). ProductId pending; direct EC writes disabled until real hardware confirms register layout. WMI thermal-policy fallback is enabled for performance modes when direct EC/MSR power-limit writes are unavailable.".to_string() }),
        model!("am1xxx_unverified", "OMEN 16 (2025) am1xxx Intel", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, supports_undervolt: true, notes: "Roadmap #26 — OMEN Gaming Laptop 16-am1xxx (2025 Intel, i9-14900HX + RTX 5070 Ti). ProductId pending community confirmation. Performance mode TDP = 90W PL1 / 130W PL2 per OGH reference behaviour. Set UserVerified=true once Product ID confirmed.".to_string() }),
        model!("8D40", "OMEN Slim 16 (2025) an0xxx", 2025, "OMEN16", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, supports_performance_modes: true, has_mux_switch: false, supports_gpu_power_boost: true, has_four_zone_rgb: false, supports_undervolt: false, notes: "GitHub #145 - OMEN Slim Gaming Laptop 16-an0xxx, ProductId 8D40, SKU 1H85302L6K. Exact conservative profile: WMI V1 fan/profile control retained (matches working family-fallback behavior), direct EC writes and independent curves disabled pending register-layout evidence, MUX/RGB/undervolt left unclaimed until this new thin-chassis line's hardware surface is confirmed. Reported Battery Care (Charge Limit) WMI failure and Performance-mode persistence are tracked separately — see 3.8.1-BUG-REPORTS.md.".to_string() }),
        model!("8603", "OMEN 17-cb0xxx (2019)", 2019, "Legacy", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: false, supports_independent_fan_curves: false, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: false, has_keyboard_backlight: true, supports_undervolt: false, notes: "GitHub #182 — HP OMEN 17-cb0xxx (i9-9880H + RTX 2080), ProductId 8603. Was resolving via family fallback and inheriting an unrelated template board's capabilities; GPU Power Boost specifically confirmed non-functional on this hardware via an independent OmenMon BIOS probe (GetGpuPower() fails with 'Command not available'). Feature flags conservative pending field verification.".to_string() }),
        model!("8BB1", "OMEN 17 (2021) Intel", 2021, "OMEN17", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: false, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "8BB1 is shared with Victus 15-fa1xxx; OMEN 17 profile selected when model name lacks 15-fa1 substring".to_string() }),
        model!("8A18", "OMEN 17-ck1xxx (2022)", 2022, "OMEN17", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: false, fan_zone_count: 2, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "GitHub #134/#144 — WMI V1 control with worker-backed CPU temperature; fan-level fallback is estimated telemetry, not physical RPM. Direct EC remains unverified.".to_string() }),
        model!("8E10", "OMEN 17-db1xxx (2025)", 2025, "OMEN17", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: true, supports_undervolt: false, notes: "GitHub #130/#171 - OMEN Gaming Laptop 17-db1xxx / ProductId 8E10 (Ryzen AI 7 350 + RTX 5070/5060), BIOS F.20. MaxFanLevel=45 per #171's Guided Fan Verification (real Max-hold ceiling, not the nominal 55 - using 55 would produce an unreachable Max-mode floor and an endless reassert loop on this board). MUX switch and GPU Power Boost conservatively false pending confirmation, not inherited from the 16-ap0xxx siblings' CPU-generation match. Direct EC and independent curves remain unverified.".to_string() }),
        model!("8C3F", "HP Victus 15-fa1xxx (2022)", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: false, has_keyboard_backlight: true, supports_undervolt: false, notes: "GitHub #125 — HP Victus 15-fa1xxx (i5-12450H / RTX 2050), ProductId 8C3F. Direct entry to avoid 8BB1 ambiguous-ID path that caused fan control delays. Same conservative Victus profile as 8BB1-VICTUS15.".to_string() }),
        model!("8BB1-VICTUS15", "HP Victus 15-fa1xxx (2022)", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: false, has_keyboard_backlight: true, supports_undervolt: false, notes: "Victus 15-fa1xxx — single-color backlight; shares 8BB1 product ID with OMEN 17 (2021)".to_string() }),
        model!("8E5E", "HP Victus 15-fa2303TX (2024)", 2024, "Victus", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: false, supports_independent_fan_curves: false, supports_rpm_readback: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: false, has_keyboard_backlight: true, supports_undervolt: false, notes: "GitHub #178 — HP Victus 15-fa2303TX / C2JQ3PA, ProductId 8E5E. Fan-verification diagnostic showed WMI fan-level control responding but RPM readback is level-estimated, not a real tachometer, and diverged from expectations under load (3/6 tests passed). Single-zone, static-color-only keyboard backlight per reporter. Feature flags conservative pending further field verification.".to_string() }),
        model!("8B9D", "OMEN 17 (2023) Intel", 2023, "OMEN17", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, has_light_bar: true }),
        model!("17CK2", "OMEN 17-ck2xxx (2023)", 2023, "OMEN17", { supports_fan_control_wmi: false, supports_fan_control_ec: true, supports_fan_curves: true, supports_independent_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, supports_advanced_optimus: true, has_four_zone_rgb: true, supports_undervolt: true, notes: "OMEN 17-ck2 series (2023) � WMI ineffective, use OGH proxy or EC access".to_string() }),
        model!("8B9E", "OMEN 17 (2023) AMD", 2023, "OMEN17", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_undervolt: false, has_four_zone_rgb: true, has_light_bar: true }),
        model!("8C3A", "OMEN Transcend 14 (2023)", 2023, "Transcend", { supports_fan_control_wmi: false, supports_fan_control_ec: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: false, has_per_key_rgb: true, notes: "Transcend uses different WMI interface - may require OGH proxy for fan control".to_string() }),
        model!("8C3B", "OMEN Transcend 16 (2023)", 2023, "Transcend", { supports_fan_control_wmi: false, supports_fan_control_ec: true, supports_fan_curves: true, has_mux_switch: true, has_four_zone_rgb: false, has_per_key_rgb: true, notes: "Transcend uses different WMI interface - may require OGH proxy for fan control".to_string() }),
        model!("8C58", "OMEN Transcend 14 (2024) fb1xxx", 2024, "Transcend", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: false, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: false, has_per_key_rgb: true, supports_undervolt: false, notes: "GitHub #149 — OMEN Transcend 14 (2024) fb0xxx/8C58. Same Transcend 14 board family as 8E41; added AllowV1AutoModeFloorClear to match 8E41 profile. Prefer hp-wmi/ACPI paths; direct legacy EC writes are unsafe on Linux and unverified on Windows.".to_string() }),
        model!("8E41", "OMEN Transcend 14 (2024) fb1xxx", 2024, "Transcend", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: false, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: false, has_per_key_rgb: true, supports_undervolt: false, notes: "GitHub #99 / Linux reports for 8E41 (Transcend 14-fb1xxx). Discord 2026-06-02 Windows field report confirms exact board identity and WMI V1 behavior; use profile-based control paths, allow V1 auto handoff floor clear, and avoid legacy EC writes or custom curves.".to_string() }),
        model!("88D9", "HP Victus 15 (2022) Intel", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, has_four_zone_rgb: false, has_keyboard_backlight: true, notes: "Victus has limited features compared to OMEN".to_string() }),
        model!("88DA", "HP Victus 15 (2022) AMD", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: false, has_keyboard_backlight: true, notes: "Victus has limited features compared to OMEN".to_string() }),
        model!("8A3E", "HP Victus 15 (2022) fb0xxx AMD", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: false, has_keyboard_backlight: true, notes: "GitHub #105 — Victus 15-fb0xxx. Conservative Victus profile (single-zone backlight).".to_string() }),
        model!("8DCD", "HP Victus 15 (8DCD)", 2024, "Victus", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, allow_decoupled_wmi_thermal_policy_fallback: true, has_four_zone_rgb: false, has_keyboard_backlight: true, notes: "GitHub #138 - Victus 15 ProductId 8DCD reports Performance mode remains EC-limited around 40W. Conservative exact profile disables direct EC writes and enables WMI thermal-policy fallback pending diagnostics/readback validation.".to_string() }),
        model!("8A26", "HP Victus 16 (2023/2024) d1xxx", 2023, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: true, has_keyboard_backlight: true, notes: "GitHub #66 — Victus 16-d1xxx (8A26). Capabilities inferred from nearby Victus 16 entries; awaiting user confirmation.".to_string() }),
        model!("8A25", "HP Victus 16 (2023/2024) d1176TX", 2023, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: true, supports_undervolt: false, has_four_zone_rgb: true, has_keyboard_backlight: true, notes: "Discord reports (ACe_Centrick, corroborated by OsamaBiden), v3.8.0/v4.0.0/v4.1.0 — RTX 3060 Laptop GPU boosts 85W base TGP -> 100W dynamic boost via WMI PPAB, matching OMEN Gaming Hub on the same hardware. Other flags inherited from the sibling 8A26 entry and remain otherwise unconfirmed for this exact board.".to_string() }),
        model!("8BD4", "HP Victus 16-s0xxx AMD", 2023, "Victus", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: true, has_keyboard_backlight: true, notes: "RC1 field log - Victus 16-s0xxx (8BD4), Ryzen 7 7840HS + RTX 4060. Conservative WMI V1 fan profile; GPU boost disabled pending verification. Discord 2026-06-08 / 7Z5Z2EA reports basic keyboard RGB should be controllable through WMI ColorTable; EC keyboard writes remain disabled. Discord 2026-06-03 reported fans stuck at max after long gaming session; v3.7.1 Discord 2026-06-07 logs showed non-reactive/0 RPM fan behavior after SetFanLevel(0,0), so V1 manual-zero floor clear is disabled pending a safer handoff sequence.".to_string() }),
        model!("8C2F", "HP Victus 15/16 (2024+) Ryzen (shared board)", 2024, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: true, notes: "GitHub #110 (16-r0xxx) + #155 (15-fb2082wm) — ProductId 8C2F is shared across the 15\" and 16\" Victus Ryzen 2024+ chassis. Capabilities were inferred from the 16\" report and are not yet confirmed on the 15\" chassis. Keyboard entry 8C2F already present in KeyboardModelDatabase. RequiredCpuVendor=AMD added after GitHub #172 (board 8BBE) showed the same \"16-r0\" WMI name pattern also matches an Intel machine, which must not inherit this AMD-only capability profile via the name-pattern fallback.".to_string() }),
        model!("88DB", "HP Victus 16 (2022)", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: true }),
        model!("88EC", "HP Victus 16-e0xxx", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_curves: true, supports_independent_fan_curves: false, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: false, has_keyboard_backlight: true, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "Issue #128 — explicit Victus 16-e0xxx mapping (88EC) to avoid low-confidence family fallback; feature flags intentionally conservative pending field verification. A follow-up diagnostics export confirmed 'Performance mode Balanced: nothing was applied (Direct EC writes disabled)' live in the field - this board never got the AllowDecoupledWmiThermalPolicyFallback fix already applied to 8DCD/8C30/878C/8600, so switching modes had no effect at all since Direct EC is (correctly) disabled here and no fallback path was enabled to compensate.".to_string() }),
        model!("88EE", "HP Victus 16-e0194nw", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: false, supports_independent_fan_curves: false, fan_zone_count: 2, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: false, has_keyboard_backlight: true, notes: "GitHub #140 - HP Victus 16-e0194nw / ProductId 88EE. Exact conservative sibling of 88EC added so model identity resolves by ProductId instead of low-confidence 16-e0 model-name pattern; feature flags remain conservative pending field verification.".to_string() }),
        model!("DESKTOP-25L", "OMEN 25L Desktop", 2021, "Desktop", { supports_fan_control_wmi: false, supports_fan_control_ec: false, supports_fan_curves: false, supports_rpm_readback: true, supports_performance_modes: true, has_mux_switch: false, supports_gpu_power_boost: false, has_keyboard_backlight: false, has_four_zone_rgb: false, notes: "OMEN 25L Desktop - fan writes disabled by v3.6.3 safety gate; RPM telemetry/performance modes only pending hardware validation.".to_string() }),
        model!("DESKTOP-30L", "OMEN 30L Desktop", 2022, "Desktop", { supports_fan_control_wmi: false, supports_fan_control_ec: false, supports_fan_curves: false, supports_rpm_readback: true, supports_performance_modes: true, has_keyboard_backlight: false, notes: "OMEN 30L Desktop - fan writes disabled by v3.6.3 safety gate; RPM telemetry/performance modes only pending hardware validation.".to_string() }),
        model!("DESKTOP-35L", "OMEN 35L Desktop", 2023, "Desktop", { supports_fan_control_wmi: false, supports_fan_control_ec: false, supports_fan_curves: false, supports_rpm_readback: true, supports_performance_modes: true, has_keyboard_backlight: false, notes: "OMEN 35L Desktop - fan writes disabled by v3.6.3 safety gate; RPM telemetry/performance modes only pending hardware validation.".to_string() }),
        model!("DESKTOP-40L", "OMEN 40L Desktop", 2023, "Desktop", { supports_fan_control_wmi: false, supports_fan_control_ec: false, supports_fan_curves: false, supports_rpm_readback: true, supports_performance_modes: true, has_keyboard_backlight: false, notes: "OMEN 40L Desktop - fan writes disabled by v3.6.3 safety gate; RPM telemetry/performance modes only pending hardware validation.".to_string() }),
        model!("DESKTOP-45L", "OMEN 45L Desktop", 2023, "Desktop", { supports_fan_control_wmi: false, supports_fan_control_ec: false, supports_fan_curves: false, supports_rpm_readback: true, supports_performance_modes: true, has_keyboard_backlight: false, notes: "OMEN 45L Desktop - fan writes disabled by v3.6.3 safety gate; RPM telemetry/performance modes only pending hardware validation.".to_string() }),
        model!("8EDD", "OMEN HyperX (2024)", 2024, "OMEN16", { supports_fan_control_wmi: true, supports_fan_curves: true, has_mux_switch: true, supports_gpu_power_boost: true, has_four_zone_rgb: true, notes: "GitHub #243".to_string() }),
        // ── New boards from community issues ──
        model!("8E5D", "HP Victus 15 (2024)", 2024, "Victus", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: true, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: false, has_keyboard_backlight: true, notes: "GitHub #245 — quickman1001 reports omen-space works normally. Community-verified entry. Conservative Victus profile (single-zone backlight, no MUX). RPM telemetry and WMI fan mode control confirmed functional.".to_string() }),
        model!("8A4F", "HP Victus 15-fa0000 (2022)", 2022, "Victus", { supports_fan_control_wmi: true, supports_fan_control_ec: false, supports_fan_curves: true, supports_independent_fan_curves: false, supports_rpm_readback: false, fan_zone_count: 1, has_mux_switch: false, supports_gpu_power_boost: false, supports_undervolt: false, has_four_zone_rgb: false, has_keyboard_backlight: true, allow_decoupled_wmi_thermal_policy_fallback: true, notes: "GitHub #246 — TrDiTu reports app works normally except fan control on Victus 15-fa0000. Conservative Victus profile; WMI thermal-policy fallback enabled. Fan mode WMI commands may not be effective on this board; RPM readback disabled pending validation. Profile (Quiet/Balanced/Performance) changes route via hp_wmi/acpi platform_profile sysfs when WMI BIOS command path is unavailable.".to_string() }),
    ])
}

fn get_known_model(board_id: &str) -> Option<ModelCapabilities> {
    let id_upper = board_id.to_uppercase();
    get_all_models().iter().find(|m| m.product_id == id_upper).cloned()
}
