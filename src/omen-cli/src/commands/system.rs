use clap::Subcommand;
use zbus::Connection;
use anyhow::Result;
use crate::dbus_proxy::PlatformProxy;

#[derive(Subcommand, Debug, Clone)]
pub enum SystemCommand {
    /// Get static hardware and system info
    Info,
    /// Apply hardware keyboard fixes
    FixKeyboard {
        #[arg(long)]
        prtsc: bool,
        #[arg(long)]
        f1: bool,
    },
    /// Set battery charge limit (50 to 100; 100 disables)
    BatteryCare {
        limit: u32,
    },
    /// Configure hysteretic battery charge limiter (50-95 stop, or 100 to disable)
    BatteryLimit {
        /// Stop charging percentage (50..=95, or 100 to disable)
        stop: u32,
        /// Resume charging percentage (20..stop)
        #[arg(long)]
        start: Option<u32>,
        /// Hold charge blocking across shutdown/reboot on WMI 0x2B boards
        #[arg(long, default_value_t = true)]
        off_hold: bool,
    },
    /// Toggle temporary one-shot full charge bypass (on/off)
    BatteryBypass {
        /// "on" or "off"
        mode: String,
    },
    /// Show live battery charge, health, and status
    Battery,
    /// Run WMI hardware diagnostics suite
    Diagnostics,
    /// Generate a hardware triage bundle for bug reports
    TriageBundle,
    /// Run fan cleaning ritual
    CleanFans,
    /// Check for conflicting daemons
    CheckConflicts,
    /// Check for BIOS updates
    CheckBios,
    /// Check for Omen Space updates
    CheckUpdate,
    /// Apply Omen Space update
    ApplyUpdate,
    /// Clear page cache memory
    CleanMemory,
}

pub async fn handle(cmd: &SystemCommand, conn: &Connection) -> Result<()> {
    let proxy = PlatformProxy::new(conn).await?;

    match cmd {
        SystemCommand::Info => {
            let res = proxy.get_hardware_dump_json().await?;
            println!("{}", res);
        }
        SystemCommand::FixKeyboard { prtsc, f1 } => {
            let res = proxy.set_keyboard_fixes(*prtsc, *f1).await?;
            println!("Response: {}", res);
        }
        SystemCommand::BatteryCare { limit } => {
            let res = proxy.set_battery_care(*limit).await?;
            println!("Response: {}", res);
        }
        SystemCommand::BatteryLimit { stop, start, off_hold } => {
            let enabled = *stop < 100;
            let s_stop = if enabled { (*stop).clamp(50, 95) } else { 100 };
            let s_start = start.unwrap_or_else(|| s_stop.saturating_sub(5).max(20));
            let res = proxy.set_battery_limit_config(enabled, s_stop, s_start, *off_hold).await?;
            println!("Response: {}", res);
        }
        SystemCommand::BatteryBypass { mode } => {
            let enabled = matches!(mode.trim().to_ascii_lowercase().as_str(), "on" | "1" | "true" | "yes");
            let res = proxy.set_battery_bypass(enabled).await?;
            println!("Response: {}", res);
        }
        SystemCommand::Battery => {
            let sysmon = crate::dbus_proxy::SysMonProxy::new(conn).await?;
            let diag_str = sysmon.get_diagnostics().await?;
            let limit_json: serde_json::Value = match proxy.get_battery_limit_state().await {
                Ok(s) => serde_json::from_str(&s).unwrap_or(serde_json::Value::Null),
                Err(_) => serde_json::Value::Null,
            };
            let json: serde_json::Value = serde_json::from_str(&diag_str).unwrap_or(serde_json::Value::Null);
            if let Some(bat) = json.get("battery").filter(|b| !b.is_null()) {
                let name = bat["name"].as_str().unwrap_or("BAT0");
                println!("\x1b[1;36m=== Battery Telemetry ({}) ===\x1b[0m", name);
                let status_raw = bat["status"].as_str().unwrap_or("unknown");
                let (status_color, status_name) = match status_raw.to_lowercase().as_str() {
                    "charging" => ("\x1b[1;32m", "Charging"),
                    "discharging" => ("\x1b[1;33m", "Discharging"),
                    "not_charging" | "not charging" => ("\x1b[1;34m", "Not charging"),
                    "full" => ("\x1b[1;34m", "Full"),
                    _ => ("\x1b[1;37m", status_raw),
                };
                let ac_online = bat["ac_online"].as_bool().unwrap_or(false);
                println!("  \x1b[1mStatus:\x1b[0m         {}{}\x1b[0m (AC Online: {})", status_color, status_name, ac_online);
                println!("  \x1b[1mCurrent Charge:\x1b[0m {}%", bat["charge_pct"].as_u64().unwrap_or(0));
                if let Some(h) = bat["health_pct"].as_f64() {
                    let health_color = if h >= 80.0 { "\x1b[1;32m" } else if h >= 60.0 { "\x1b[1;33m" } else { "\x1b[1;31m" };
                    println!("  \x1b[1mBattery Health:\x1b[0m {}{:.1}%\x1b[0m", health_color, h);
                }
                if let (Some(now), Some(full)) = (bat["energy_now_wh"].as_f64(), bat["energy_full_wh"].as_f64()) {
                    let des_str = bat["energy_design_wh"].as_f64().map(|d| format!(" / {:.2} Wh design", d)).unwrap_or_default();
                    println!("  \x1b[1mCapacity:\x1b[0m       {:.2} / {:.2} Wh{}", now, full, des_str);
                }
                if let Some(cycles) = bat["cycle_count"].as_u64() {
                    println!("  \x1b[1mCycle Count:\x1b[0m    {}", cycles);
                }
                let power_w = bat["power_w"].as_f64().unwrap_or(0.0);
                if power_w > 0.0 {
                    println!("  \x1b[1mPower Rate:\x1b[0m     {:.2} W", power_w);
                }
                if let Some(v) = bat["voltage_v"].as_f64() {
                    println!("  \x1b[1mVoltage:\x1b[0m        {:.2} V", v);
                }
                if let Some(lim) = bat["charge_limit_pct"].as_u64() {
                    println!("  \x1b[1mCare Limit:\x1b[0m     {}%", lim);
                }
                if limit_json["supported"].as_bool().unwrap_or(false) {
                    let backend = limit_json["backend"].as_str().unwrap_or("unknown");
                    let enabled = limit_json["enabled"].as_bool().unwrap_or(false);
                    let stop = limit_json["stop"].as_u64().unwrap_or(100);
                    let start = limit_json["start"].as_u64().unwrap_or(75);
                    let state = limit_json["limiter_state"].as_str().unwrap_or("disabled");
                    let bypass = limit_json["bypass"].as_bool().unwrap_or(false);
                    let off_hold = limit_json["off_hold"].as_bool().unwrap_or(true);
                    let fw = limit_json["firmware_mode"].as_str().unwrap_or("");
                    if enabled {
                        println!(
                            "  \x1b[1mCharge Limiter:\x1b[0m {} ({}%..{}%, off_hold={}, bypass={}, backend={}{})",
                            state,
                            start,
                            stop,
                            off_hold,
                            bypass,
                            backend,
                            if fw.is_empty() { String::new() } else { format!(", fw={}", fw) }
                        );
                    } else {
                        println!("  \x1b[1mCharge Limiter:\x1b[0m off (backend={})", backend);
                    }
                }
            } else {
                println!("No battery detected (desktop system or battery not reporting).");
            }
        }
        SystemCommand::Diagnostics => {
            println!("Running WMI Diagnostics. This may take a few seconds...");
            let res = proxy.run_wmi_diagnostics().await?;
            println!("Report:\n{}", res);
        }
        SystemCommand::TriageBundle => {
            println!("Generating triage bundle...");
            let res = proxy.generate_triage_bundle().await?;
            println!("Archive path: {}", res);
        }
        SystemCommand::CleanFans => {
            println!("Starting fan cleaning routine. Fans will max out for a few seconds.");
            let res = proxy.run_fan_cleaning().await?;
            println!("Response: {}", res);
        }
        SystemCommand::CheckConflicts => {
            let res = proxy.check_conflicts().await?;
            println!("Conflicts:\n{}", res);
        }
        SystemCommand::CheckBios => {
            println!("Checking HP Catalog for BIOS updates...");
            let res = proxy.check_bios_update().await?;
            println!("BIOS Update Info:\n{}", res);
        }
        SystemCommand::CheckUpdate => {
            println!("Checking GitHub for Omen Space updates...");
            let res = proxy.check_app_update().await?;
            println!("Update Info:\n{}", res);
        }
        SystemCommand::ApplyUpdate => {
            println!("Applying Omen Space update...");
            let res = proxy.apply_app_update().await?;
            println!("Response:\n{}", res);
        }
        SystemCommand::CleanMemory => {
            let res = proxy.clean_memory().await?;
            println!("Response: {}", res);
        }
    }

    Ok(())
}
