use serde::{Deserialize, Serialize};

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
    /// Minimum nonzero manual duty (0..255), distinct from firmware fan-stop support.
    #[serde(default = "default_minimum_manual_pwm")]
    pub minimum_manual_pwm: u32,

    /// Delegate Auto fan mode to the firmware's native EC curve instead of holding
    /// manual WMI `pwm1_enable=1` constantly, preventing EC thermal-guard max-fan latches (#395).
    #[serde(default)]
    pub prefer_ec_auto_fan: bool,

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
    /// Physical keyboard lighting zones when the board is single-zone (1). The
    /// hp-omen-extra driver exposes zone0..zone7 on every board, so this can't be
    /// detected from sysfs; None keeps the existing 4/8-zone logic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rgb_zone_count: Option<u32>,
    /// True when the board's backlight is non-WMI (e.g. USB HID only) and
    /// probing/writing WMI 0x20009 (hp-omen-extra) shuts down the EC rail (#394).
    #[serde(default)]
    pub disable_wmi_rgb: bool,

    // Power / Undervolt
    pub supports_undervolt: bool,
    pub supports_tcc_offset: bool,
    pub supports_power_limits: bool,
    pub supports_battery_care: bool,

    pub notes: String,
}

fn default_minimum_manual_pwm() -> u32 {
    50
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
            minimum_manual_pwm: default_minimum_manual_pwm(),
            prefer_ec_auto_fan: false,

            supports_performance_modes: true,
            performance_modes: vec![
                "Default".to_string(),
                "Performance".to_string(),
                "Cool".to_string(),
            ],
            allow_decoupled_wmi_thermal_policy_fallback: false,

            has_mux_switch: false,
            supports_gpu_power_boost: true,
            supports_advanced_optimus: false,

            has_keyboard_backlight: true,
            has_four_zone_rgb: true,
            has_per_key_rgb: false,
            has_light_bar: false,
            rgb_zone_count: None,
            disable_wmi_rgb: false,

            supports_undervolt: true,
            supports_tcc_offset: true,
            supports_power_limits: true,
            supports_battery_care: true,

            notes: "".to_string(),
        }
    }
}

pub struct LinuxCapabilityClassifier;

impl LinuxCapabilityClassifier {
    pub fn is_wmaa_abort_prone_board(board_id: &str) -> bool {
        let id_upper = board_id.trim().to_uppercase();
        get_hardware_database()
            .wmaa_abort_prone_boards
            .contains(&id_upper)
    }

    /// Returns true when the board_id appears in the community-verified list.
    ///
    /// A "verified" board is one where a user has run the full checklist
    /// (modes, fans, power gain, GPU power, graphics, lighting) and confirmed
    /// every control behaves as expected.  Boards not in this list still receive
    /// full daemon service; the GUI simply shows a one-time banner asking the
    /// user to file a verification issue.
    pub fn is_board_verified(board_id: &str) -> bool {
        let id_upper = board_id.trim().to_uppercase();
        get_hardware_database().verified_boards.contains(&id_upper)
    }
}

pub fn detect(board_id: &str, product_name: &str, cpu_model: &str) -> ModelCapabilities {
    let mut cap = get_known_model(board_id)
        .or_else(|| {
            let prod_lower = product_name.to_lowercase();
            // Fallback by product name if exact board_id is not found
            get_all_models()
                .iter()
                .find(|m| prod_lower.contains(&m.model_name.to_lowercase()))
                .cloned()
        })
        .unwrap_or_default();

    let cpu_upper = cpu_model.to_uppercase();
    let is_hx = cpu_upper.contains("HX");
    let is_amd = cpu_upper.contains("AMD") || cpu_upper.contains("RYZEN");

    if cap.family.to_uppercase().contains("VICTUS") && !is_hx && !is_amd {
        cap.supports_undervolt = false;
        cap.supports_tcc_offset = false;
        cap.supports_power_limits = false;
    }
    cap
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HardwareDatabase {
    pub models: Vec<ModelCapabilities>,
    pub verified_boards: Vec<String>,
    pub wmaa_abort_prone_boards: Vec<String>,
}

fn get_hardware_database() -> &'static HardwareDatabase {
    static DB: std::sync::OnceLock<HardwareDatabase> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        serde_json::from_str(include_str!("boards.json")).unwrap_or_else(|e| {
            log::error!("Failed to parse boards.json: {}", e);
            HardwareDatabase {
                models: Vec::new(),
                verified_boards: Vec::new(),
                wmaa_abort_prone_boards: Vec::new(),
            }
        })
    })
}

fn get_all_models() -> &'static [ModelCapabilities] {
    &get_hardware_database().models
}

fn get_known_model(board_id: &str) -> Option<ModelCapabilities> {
    let id_upper = board_id.to_uppercase();
    get_all_models()
        .iter()
        .find(|m| m.product_id == id_upper)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_8786_capabilities() {
        let cap = detect("8786", "OMEN Laptop 15-en0xxx", "AMD Ryzen 5 4600H");
        assert_eq!(cap.product_id, "8786");
        assert!(cap.supports_performance_modes);
        assert!(cap.allow_decoupled_wmi_thermal_policy_fallback);
        assert!(!cap.supports_undervolt); // AMD should not have undervolt enabled
        assert!(LinuxCapabilityClassifier::is_wmaa_abort_prone_board("8786"));
    }

    #[test]
    fn test_board_8edf_disables_wmi_rgb() {
        let cap = detect(
            "8EDF",
            "HyperX OMEN Gaming Laptop 15-ga0xxx",
            "Intel Core Ultra 7 251HX",
        );
        assert_eq!(cap.product_id, "8EDF");
        assert!(
            cap.disable_wmi_rgb,
            "8EDF must disable WMI RGB probing (#394)"
        );
        assert!(!cap.has_four_zone_rgb);
        assert!(!cap.has_per_key_rgb);
        assert!(!cap.has_light_bar);
        assert!(cap.has_mux_switch);
        assert!(cap.supports_fan_control_wmi);
        assert!(cap.supports_performance_modes);
        assert!(LinuxCapabilityClassifier::is_board_verified("8EDF"));
        assert!(LinuxCapabilityClassifier::is_board_verified("8D42"));
    }
}

#[cfg(test)]
mod fan_minimum_tests {
    #[test]
    fn stall_workaround_does_not_change_confirmed_low_speed_boards() {
        for (board, minimum) in [("8EEF", 89), ("8D41", 50), ("8A17", 50)] {
            assert_eq!(
                super::get_known_model(board).unwrap().minimum_manual_pwm,
                minimum
            );
        }
        assert_eq!(super::ModelCapabilities::default().minimum_manual_pwm, 50);
    }
}
