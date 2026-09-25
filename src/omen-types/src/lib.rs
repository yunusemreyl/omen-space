use serde::{Deserialize, Serialize};
use zbus::proxy;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_temp: i32,
    pub cpu_load: f64,
    pub cpu_pwr: f64,
    pub fan_rpm: i32,
    pub fan1_rpm: i32,
    pub fan2_rpm: i32,
    pub gpu_temp: i32,
    pub gpu_load: f64,
    pub gpu_pwr: f64,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_frac: f64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_frac: f64,
    pub total_pwr: f64,
    pub cpu_throttle_count: u32,
    #[serde(default)]
    pub chassis_temp: i32,
    #[serde(default)]
    pub board_verified: bool,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSpecs {
    pub product_name: String,
    pub cpu_spec: String,
    pub gpu_spec: String,
    pub ram_spec: String,
    pub ssd_spec: String,
    pub os_spec: String,
    pub bios_version: String,
    pub ec_version: String,
    pub vbios_version: String,
    pub nvidia_driver: String,
    pub kernel_version: String,
}

// ── Power Service Proxy ────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
pub trait Power {
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn get_power_profile(&self) -> zbus::Result<String>;
    async fn set_power_limits(&self, enabled: bool, pl1: i32, pl2: i32) -> zbus::Result<String>;
    async fn set_app_profiles_enabled(&self, enabled: bool) -> zbus::Result<String>;
}

// ── Fan Service Proxy ─────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
pub trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_fan_mode(&self) -> zbus::Result<String>;
    async fn get_fan_info(&self) -> zbus::Result<String>;
    async fn save_custom_curve(&self, curve_json: &str) -> zbus::Result<String>;
    async fn set_thermal_protection(&self, enabled: bool) -> zbus::Result<String>;
    
    #[zbus(signal)]
    fn thermal_protection_alert(&self, active: bool) -> zbus::Result<()>;
}

// ── SysMon Service Proxy ──────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.SysMon",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/SysMon"
)]
pub trait SysMon {
    async fn get_diagnostics(&self) -> zbus::Result<String>;
    async fn get_hardware_specs(&self) -> zbus::Result<String>;
    async fn generate_diagnostic_report(&self) -> zbus::Result<String>;
    async fn generate_rgb_issue(&self) -> zbus::Result<String>;
    
    #[zbus(signal)]
    fn telemetry_updated(&self, json_stats: &str) -> zbus::Result<()>;
}

// ── Rgb Service Proxy ─────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Rgb",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Rgb"
)]
pub trait Rgb {
    async fn set_color(&self, zone_val: i32, hex_color: &str) -> zbus::Result<String>;
    async fn set_mode(&self, mode_str: &str, speed_val: i32) -> zbus::Result<String>;
    async fn set_global(&self, power_val: bool, brightness_val: i32, direction_str: &str) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn set_per_key_colors(&self, colors_json: &str) -> zbus::Result<String>;
    async fn start_per_key_wizard(&self) -> zbus::Result<String>;
    async fn light_key_index(&self, index: u32, hex_color: &str) -> zbus::Result<String>;
    async fn record_key_mapping(&self, index: u32, key_name: &str) -> zbus::Result<String>;
    async fn export_keymap_report(&self) -> zbus::Result<String>;
}

// ── Platform Service Proxy ───────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Platform",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Platform"
)]
pub trait Platform {
    async fn set_battery_care(&self, limit: u32) -> zbus::Result<String>;
    async fn run_fan_cleaning(&self) -> zbus::Result<String>;
    async fn toggle_overlay(&self) -> zbus::Result<String>;
    async fn generate_triage_bundle(&self) -> zbus::Result<String>;
    #[zbus(signal)]
    fn macro_key_pressed(&self, key_name: &str) -> zbus::Result<()>;
}

// ── Mux Service Proxy ────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Mux",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Mux"
)]
pub trait Mux {
    async fn set_gpu_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_gpu_info(&self) -> zbus::Result<String>;
}

// ── Undervolt Service Proxy ──────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Undervolt",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Undervolt"
)]
pub trait Undervolt {
    async fn set_offset(&self, plane: &str, offset_mv: i32) -> zbus::Result<String>;
    async fn set_tcc_offset(&self, val: i32) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
}

// ── AppProfiles Service Proxy ──────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.AppProfiles",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/AppProfiles"
)]
pub trait AppProfiles {
    async fn get_profiles(&self) -> zbus::Result<String>;
    async fn add_profile(&self, process_name: &str, power_profile: &str, fan_mode: &str) -> zbus::Result<String>;
    async fn remove_profile(&self, process_name: &str) -> zbus::Result<String>;
}
