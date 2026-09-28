use zbus::proxy;

#[proxy(
    interface = "org.hp.omen.Rgb",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Rgb"
)]
pub trait Rgb {
    async fn set_mode(&self, mode_str: &str, speed_val: i32) -> zbus::Result<String>;
    async fn set_color(&self, zone_val: i32, hex_color: &str) -> zbus::Result<String>;
    async fn set_global(&self, power_val: bool, brightness_val: i32, direction_str: &str) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn set_per_key_colors(&self, colors_json: &str) -> zbus::Result<String>;
    async fn test_single_key(&self, index: i32) -> zbus::Result<String>;
    async fn start_per_key_wizard(&self) -> zbus::Result<String>;
    async fn light_wizard_key(&self, index: u32, hex_color: &str) -> zbus::Result<String>;
    async fn record_wizard_key(&self, index: u32, label: &str) -> zbus::Result<String>;
    async fn export_wizard_keymap(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
pub trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn set_fan_target(&self, fan_num: u32, rpm: u32) -> zbus::Result<String>;
    async fn get_fan_mode(&self) -> zbus::Result<String>;
    async fn get_fan_info(&self) -> zbus::Result<String>;
    async fn save_custom_curve(&self, curve_json: &str) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
pub trait Power {
    async fn get_power_profile(&self) -> zbus::Result<String>;
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn set_power_limits(&self, enabled: bool, pl1: i32, pl2: i32) -> zbus::Result<String>;
    async fn set_undervolt(&self, mv: i32) -> zbus::Result<String>;
    async fn set_tcc_offset(&self, val: i32) -> zbus::Result<String>;
    async fn set_app_profiles_enabled(&self, enabled: bool) -> zbus::Result<String>;
    async fn set_app_profiles(&self, profiles_json: &str) -> zbus::Result<String>;
    async fn get_ac_auto_performance(&self) -> zbus::Result<bool>;
    async fn set_ac_auto_performance(&self, enabled: bool) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.Mux",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Mux"
)]
pub trait Mux {
    async fn set_gpu_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_gpu_info(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.Undervolt",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Undervolt"
)]
pub trait Undervolt {
    async fn set_offset(&self, plane: &str, offset_mv: i32) -> zbus::Result<String>;
    async fn get_status(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.Platform",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Platform"
)]
pub trait Platform {
    async fn get_system_info(&self) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn set_keyboard_fixes(&self, prtsc: bool, f1: bool) -> zbus::Result<String>;
    async fn set_battery_care(&self, limit: u32) -> zbus::Result<String>;
    async fn clean_memory(&self) -> zbus::Result<String>;
    async fn generate_hardware_dump(&self) -> zbus::Result<String>;
    async fn get_hardware_dump_json(&self) -> zbus::Result<String>;
    async fn run_wmi_diagnostics(&self) -> zbus::Result<String>;
    async fn run_fan_cleaning(&self) -> zbus::Result<String>;
    async fn check_conflicts(&self) -> zbus::Result<String>;
    async fn analyze_acpi(&self) -> zbus::Result<String>;
    async fn generate_triage_bundle(&self) -> zbus::Result<String>;
    async fn check_bios_update(&self) -> zbus::Result<String>;
    async fn check_app_update(&self) -> zbus::Result<String>;
    async fn apply_app_update(&self) -> zbus::Result<String>;
    async fn toggle_overlay(&self) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.SysMon",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/SysMon"
)]
pub trait SysMon {
    async fn get_diagnostics(&self) -> zbus::Result<String>;
}
