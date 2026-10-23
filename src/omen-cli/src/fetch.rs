use anyhow::Result;
use serde_json::Value;
use std::env;
use std::io::{stdout, Write};
use std::collections::VecDeque;
use std::time::Duration;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::dbus_proxy::{PlatformProxy, FanProxy, PowerProxy, RgbProxy, MuxProxy, SysMonProxy};

pub async fn run_live_dashboard(conn: &zbus::Connection) -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, cursor::Hide)?;

    let platform = PlatformProxy::new(conn).await?;
    let fan = FanProxy::new(conn).await?;
    let power = PowerProxy::new(conn).await?;
    let rgb = RgbProxy::new(conn).await?;
    let mux = MuxProxy::new(conn).await.ok();
    let sysmon = SysMonProxy::new(conn).await?;

    let mut input = String::new();
    let mut logs: VecDeque<String> = VecDeque::with_capacity(3);
    logs.push_back(format!("\x1b[1;36m[SYSTEM]\x1b[0m {}", crate::i18n::t("ready")));

    let user = env::var("USER").unwrap_or_else(|_| "user".to_string());
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_else(|_| "omen-laptop".to_string())
        .trim()
        .to_string();

    let mut last_tick = tokio::time::Instant::now();
    let mut need_redraw = true;

    // Initial state fetch
    let sys_json: Value = serde_json::from_str(&platform.get_hardware_dump_json().await.unwrap_or_default()).unwrap_or(Value::Null);
    let mut fan_json: Value = serde_json::from_str(&fan.get_fan_info().await.unwrap_or_default()).unwrap_or(Value::Null);
    let mut power_json: Value = serde_json::from_str(&power.get_power_profile().await.unwrap_or_default()).unwrap_or(Value::Null);
    let mut rgb_json: Value = serde_json::from_str(&rgb.get_state().await.unwrap_or_default()).unwrap_or(Value::Null);
    let mut state_json: Value = serde_json::from_str(&platform.get_state().await.unwrap_or_default()).unwrap_or(Value::Null);
    let conflicts_json: Value = serde_json::from_str(&platform.check_conflicts().await.unwrap_or_default()).unwrap_or(Value::Null);
    let mut sysmon_json: Value = serde_json::from_str(&sysmon.get_diagnostics().await.unwrap_or_default()).unwrap_or(Value::Null);

    let os_name = get_os_release_name().unwrap_or_else(|| "Linux".to_string());

    let ascii_logo = [
        "\x1b[38;2;0;162;255m         .::.         \x1b[0m",
        "\x1b[38;2;20;140;255m       .::::::.       \x1b[0m",
        "\x1b[38;2;40;120;255m     .::::::::::.     \x1b[0m",
        "\x1b[38;2;60;100;255m   .::::::::::::::.   \x1b[0m",
        "\x1b[38;2;80;80;255m  .::::::::::::::::.  \x1b[0m",
        "\x1b[38;2;110;60;255m :::::::::::::::::::: \x1b[0m",
        "\x1b[38;2;140;40;255m :::::::::::::::::::: \x1b[0m",
        "\x1b[38;2;170;20;255m  '::::::::::::::::'  \x1b[0m",
        "\x1b[38;2;200;0;255m   '::::::::::::::'   \x1b[0m",
        "\x1b[38;2;170;20;255m     '::::::::::'     \x1b[0m",
        "\x1b[38;2;140;40;255m       '::::::'       \x1b[0m",
        "\x1b[38;2;0;162;255m         '::'         \x1b[0m",
    ];

    loop {
        if need_redraw {
            need_redraw = false;

            let product_name = sys_json["system"]["product_name"].as_str().unwrap_or("HP OMEN Laptop");
            let board_id = sys_json["system"]["board_id"].as_str().unwrap_or("8BBE");
            let kernel = sys_json["system"]["kernel"].as_str().unwrap_or("Linux");
            let cpu_name = sys_json["system"]["cpu_name"].as_str().unwrap_or("Processor");

            let power_active = power_json["active"].as_str().unwrap_or("balanced");
            let pl1 = power_json["pl1_w"].as_i64().unwrap_or(0);
            let pl2 = power_json["pl2_w"].as_i64().unwrap_or(0);
            let undervolt_mv = power_json["undervolt_mv"].as_i64().unwrap_or(0);
            let gpu_w = power_json["gpu_w"].as_u64().unwrap_or(0);

            let fan_mode = fan_json["mode"].as_str().unwrap_or("auto");
            let fan1_rpm = fan_json["fans"]["1"]["current"].as_u64().unwrap_or(0);
            let fan2_rpm = fan_json["fans"]["2"]["current"].as_u64().unwrap_or(0);

            let battery_limit = state_json["battery_charge_limit"].as_u64().unwrap_or(100);
            let rgb_mode = rgb_json["mode"].as_str().unwrap_or("static");
            let conflict_clean = !conflicts_json["has_conflicts"].as_bool().unwrap_or(false);

            let mux_mode = if let Some(ref m) = mux {
                let info_str = m.get_gpu_info().await.unwrap_or_default();
                let info_json: Value = serde_json::from_str(&info_str).unwrap_or(Value::Null);
                
                let mut display_mode = info_json["mode"].as_str().unwrap_or("Hybrid").to_string();
                // capitalize first letter
                if let Some(r) = display_mode.get_mut(0..1) {
                    r.make_ascii_uppercase();
                }
                display_mode
            } else {
                "Hybrid".to_string()
            };

            let user_host_header = format!("\x1b[1;38;2;0;200;255m{}\x1b[0m@\x1b[1;38;2;180;0;255m{}\x1b[0m", user, hostname);
            let separator = "\x1b[38;2;100;50;255m---------------------------------------\x1b[0m";

            let cpu_temp = sysmon_json["cpu_temp"].as_f64().unwrap_or(0.0).round() as u32;
            let gpu_temp = sysmon_json["gpu_temp"].as_f64().unwrap_or(0.0).round() as u32;
            let info_lines = vec![
                user_host_header,
                separator.to_string(),
                format!("\x1b[1;36m{}\x1b[0m: {} | \x1b[1;36m{}\x1b[0m: {}", crate::i18n::t("os"), os_name, crate::i18n::t("kernel"), kernel),
                format!("\x1b[1;36m{}\x1b[0m: {} ({})", crate::i18n::t("host"), product_name, board_id),
                format!("\x1b[1;36mCPU\x1b[0m: {}", cpu_name),
                format!("\x1b[1;36m{}\x1b[0m: \x1b[1;32m{}\x1b[0m [PL1: {}W / PL2: {}W]", crate::i18n::t("power_profile"), power_active, pl1, pl2),
                format!("\x1b[1;36m{}\x1b[0m: CPU: {}°C | GPU: {}°C | Fan1: {} RPM | Fan2: {} RPM [\x1b[33m{}\x1b[0m]", crate::i18n::t("thermal_fans"), cpu_temp, gpu_temp, fan1_rpm, fan2_rpm, fan_mode),
                format!("\x1b[1;36m{}\x1b[0m: \x1b[1;35m{}\x1b[0m | \x1b[1;31mTGP\x1b[0m: {}W | \x1b[1;36mUV\x1b[0m: {}mV", crate::i18n::t("gpu_mux"), mux_mode, gpu_w, undervolt_mv),
                if let Some(bat) = sysmon_json.get("battery").filter(|b| !b.is_null()) {
                    let charge = bat["charge_pct"].as_u64().unwrap_or(0);
                    let health_str = bat["health_pct"].as_f64().map(|h| format!("{:.0}%", h)).unwrap_or_else(|| "N/A".into());
                    let status_str = bat["status"].as_str().unwrap_or("Unknown");
                    let limit_str = if battery_limit < 100 { format!(" (Care: {}%)", battery_limit) } else { String::new() };
                    format!("\x1b[1;36mBattery\x1b[0m: {}% [{}] | \x1b[1;36mHealth\x1b[0m: {}{}", charge, status_str, health_str, limit_str)
                } else {
                    format!("\x1b[1;36m{}\x1b[0m: {}% {}", crate::i18n::t("battery_care"), battery_limit, crate::i18n::t("limit"))
                },
                format!("\x1b[1;36mRGB\x1b[0m: {}", rgb_mode),
                format!("\x1b[1;36m{}\x1b[0m: {}", crate::i18n::t("conflicts"), if conflict_clean { format!("\x1b[32m{}\x1b[0m", crate::i18n::t("clean")) } else { format!("\x1b[31m{}\x1b[0m", crate::i18n::t("warning")) }),
                "\x1b[40m   \x1b[41m   \x1b[42m   \x1b[43m   \x1b[44m   \x1b[45m   \x1b[46m   \x1b[47m   \x1b[0m".to_string(),
                "\x1b[100m   \x1b[101m   \x1b[102m   \x1b[103m   \x1b[104m   \x1b[105m   \x1b[106m   \x1b[107m   \x1b[0m".to_string(),
            ];

            let mut row_idx: u16 = 1;
            let max_lines = ascii_logo.len().max(info_lines.len());
            for i in 0..max_lines {
                let logo_part = ascii_logo.get(i).copied().unwrap_or("                      ");
                let info_part = info_lines.get(i).map(|s| s.as_str()).unwrap_or("");
                execute!(out, cursor::MoveTo(2, row_idx))?;
                write!(out, "{}   {}\x1b[K", logo_part, info_part)?;
                row_idx += 1;
            }

            // Compact Cheatsheet
            execute!(out, cursor::MoveTo(0, row_idx))?;
            write!(out, "\x1b[1;30m--------------------------------------------------------------------------------\x1b[0m\x1b[K")?;
            row_idx += 1;

            execute!(out, cursor::MoveTo(2, row_idx))?;
            write!(out, "\x1b[1;36mFan\x1b[0m: \x1b[32mfan auto|ec|max|50\x1b[0m | \x1b[1;36mPower\x1b[0m: \x1b[32mperf perf|bal|eco\x1b[0m | \x1b[1;36mMUX\x1b[0m: \x1b[32mmux hybrid|discrete\x1b[0m\x1b[K")?;
            row_idx += 1;

            execute!(out, cursor::MoveTo(2, row_idx))?;
            write!(out, "\x1b[1;36mRGB\x1b[0m: \x1b[32mrgb red|blue|off\x1b[0m | \x1b[1;36mBat\x1b[0m: \x1b[32mbat 80\x1b[0m | \x1b[1;36mUtils\x1b[0m: \x1b[32muv -50\x1b[0m | \x1b[32mclean\x1b[0m | \x1b[32mdiag\x1b[0m | \x1b[32mexit\x1b[0m\x1b[K")?;
            row_idx += 1;

            // Notification / Log Area
            execute!(out, cursor::MoveTo(0, row_idx))?;
            write!(out, "\x1b[1;30m--------------------------------------------------------------------------------\x1b[0m\x1b[K")?;
            row_idx += 1;

            execute!(out, cursor::MoveTo(0, row_idx))?;
            write!(out, "\x1b[1;33mExecution Log & Notifications:\x1b[0m\x1b[K")?;
            row_idx += 1;

            for log in &logs {
                execute!(out, cursor::MoveTo(2, row_idx))?;
                write!(out, "{}\x1b[K", log)?;
                row_idx += 1;
            }

            execute!(out, cursor::MoveTo(0, row_idx))?;
            write!(out, "\x1b[1;30m--------------------------------------------------------------------------------\x1b[0m\x1b[K")?;
            row_idx += 1;

            execute!(out, cursor::MoveTo(0, row_idx))?;
            write!(out, "\x1b[1;31momen-cli\x1b[0m \x1b[1;32m>\x1b[0m {}\x1b[K", input)?;
            out.flush()?;
        }

        // Input Polling
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char(c) => {
                        input.push(c);
                        need_redraw = true;
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        need_redraw = true;
                    }
                    KeyCode::Esc => break,
                    KeyCode::Enter => {
                        let cmd_str = input.trim().to_string();
                        input.clear();

                        if cmd_str == "exit" || cmd_str == "quit" {
                            break;
                        }

                        if !cmd_str.is_empty() {
                            let result_log = execute_input_command(&cmd_str, conn).await;
                            if logs.len() >= 3 { logs.pop_front(); }
                            logs.push_back(result_log);

                            // Refresh state immediately after command execution
                            fan_json = serde_json::from_str(&fan.get_fan_info().await.unwrap_or_default()).unwrap_or(Value::Null);
                            power_json = serde_json::from_str(&power.get_power_profile().await.unwrap_or_default()).unwrap_or(Value::Null);
                            rgb_json = serde_json::from_str(&rgb.get_state().await.unwrap_or_default()).unwrap_or(Value::Null);
                            state_json = serde_json::from_str(&platform.get_state().await.unwrap_or_default()).unwrap_or(Value::Null);
                            sysmon_json = serde_json::from_str(&sysmon.get_diagnostics().await.unwrap_or_default()).unwrap_or(Value::Null);
                            need_redraw = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Periodic telemetry refresh every 1s
        if last_tick.elapsed() >= Duration::from_millis(1000) {
            last_tick = tokio::time::Instant::now();
            fan_json = serde_json::from_str(&fan.get_fan_info().await.unwrap_or_default()).unwrap_or(Value::Null);
            power_json = serde_json::from_str(&power.get_power_profile().await.unwrap_or_default()).unwrap_or(Value::Null);
            rgb_json = serde_json::from_str(&rgb.get_state().await.unwrap_or_default()).unwrap_or(Value::Null);
            state_json = serde_json::from_str(&platform.get_state().await.unwrap_or_default()).unwrap_or(Value::Null);
            sysmon_json = serde_json::from_str(&sysmon.get_diagnostics().await.unwrap_or_default()).unwrap_or(Value::Null);
            need_redraw = true;
        }
    }

    disable_raw_mode()?;
    execute!(out, LeaveAlternateScreen, cursor::Show)?;
    Ok(())
}

pub async fn print_omen_fetch(conn: &zbus::Connection) -> Result<()> {
    let platform = PlatformProxy::new(conn).await?;
    let fan = FanProxy::new(conn).await?;
    let power = PowerProxy::new(conn).await?;
    let rgb = RgbProxy::new(conn).await?;
    let mux = MuxProxy::new(conn).await.ok();
    let sysmon = SysMonProxy::new(conn).await?;

    let sys_str = platform.get_hardware_dump_json().await.unwrap_or_default();
    let sys_json: Value = serde_json::from_str(&sys_str).unwrap_or(Value::Null);

    let fan_str = fan.get_fan_info().await.unwrap_or_default();
    let fan_json: Value = serde_json::from_str(&fan_str).unwrap_or(Value::Null);

    let power_str = power.get_power_profile().await.unwrap_or_default();
    let power_json: Value = serde_json::from_str(&power_str).unwrap_or(Value::Null);

    let rgb_str = rgb.get_state().await.unwrap_or_default();
    let rgb_json: Value = serde_json::from_str(&rgb_str).unwrap_or(Value::Null);

    let state_str = platform.get_state().await.unwrap_or_default();
    let state_json: Value = serde_json::from_str(&state_str).unwrap_or(Value::Null);

    let conflicts_str = platform.check_conflicts().await.unwrap_or_default();
    let conflicts_json: Value = serde_json::from_str(&conflicts_str).unwrap_or(Value::Null);

    let sysmon_str = sysmon.get_diagnostics().await.unwrap_or_default();
    let sysmon_json: Value = serde_json::from_str(&sysmon_str).unwrap_or(Value::Null);

    let user = env::var("USER").unwrap_or_else(|_| "user".to_string());
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_else(|_| "omen-laptop".to_string())
        .trim()
        .to_string();

    let product_name = sys_json["system"]["product_name"].as_str().unwrap_or("HP OMEN Laptop");
    let board_id = sys_json["system"]["board_id"].as_str().unwrap_or("8BBE");
    let kernel = sys_json["system"]["kernel"].as_str().unwrap_or("Linux");
    let cpu_name = sys_json["system"]["cpu_name"].as_str().unwrap_or("Processor");

    let os_name = get_os_release_name().unwrap_or_else(|| "Linux".to_string());

    let power_active = power_json["active"].as_str().unwrap_or("balanced");
    let pl1 = power_json["pl1_w"].as_i64().unwrap_or(0);
    let pl2 = power_json["pl2_w"].as_i64().unwrap_or(0);
    let undervolt_mv = power_json["undervolt_mv"].as_i64().unwrap_or(0);
            let gpu_w = power_json["gpu_w"].as_u64().unwrap_or(0);

    let fan_mode = fan_json["mode"].as_str().unwrap_or("auto");
    let fan1_rpm = fan_json["fans"]["1"]["current"].as_u64().unwrap_or(0);
    let fan2_rpm = fan_json["fans"]["2"]["current"].as_u64().unwrap_or(0);

    let battery_limit = state_json["battery_charge_limit"].as_u64().unwrap_or(100);

    let rgb_mode = rgb_json["mode"].as_str().unwrap_or("static");

    let conflict_clean = !conflicts_json["has_conflicts"].as_bool().unwrap_or(false);

    let mux_mode = if let Some(ref m) = mux {
        let info_str = m.get_gpu_info().await.unwrap_or_default();
        let info_json: Value = serde_json::from_str(&info_str).unwrap_or(Value::Null);
        
        let mut display_mode = info_json["mode"].as_str().unwrap_or("Hybrid").to_string();
        // capitalize first letter
        if let Some(r) = display_mode.get_mut(0..1) {
            r.make_ascii_uppercase();
        }
        display_mode
    } else {
        "Hybrid".to_string()
    };

    let user_host_header = format!("\x1b[1;38;2;0;200;255m{}\x1b[0m@\x1b[1;38;2;180;0;255m{}\x1b[0m", user, hostname);
    let separator = "\x1b[38;2;100;50;255m---------------------------------------\x1b[0m";

    let cpu_temp = sysmon_json["cpu_temp"].as_f64().unwrap_or(0.0).round() as u32;
    let gpu_temp = sysmon_json["gpu_temp"].as_f64().unwrap_or(0.0).round() as u32;
    let info_lines = vec![
        user_host_header,
        separator.to_string(),
        format!("\x1b[1;36m{}\x1b[0m: {} | \x1b[1;36m{}\x1b[0m: {}", crate::i18n::t("os"), os_name, crate::i18n::t("kernel"), kernel),
        format!("\x1b[1;36m{}\x1b[0m: {} ({})", crate::i18n::t("host"), product_name, board_id),
        format!("\x1b[1;36mCPU\x1b[0m: {}", cpu_name),
        format!("\x1b[1;36m{}\x1b[0m: \x1b[1;32m{}\x1b[0m [PL1: {}W / PL2: {}W]", crate::i18n::t("power_profile"), power_active, pl1, pl2),
        format!("\x1b[1;36m{}\x1b[0m: CPU: {}°C | GPU: {}°C | Fan1: {} RPM | Fan2: {} RPM [\x1b[33m{}\x1b[0m]", crate::i18n::t("thermal_fans"), cpu_temp, gpu_temp, fan1_rpm, fan2_rpm, fan_mode),
        format!("\x1b[1;36m{}\x1b[0m: \x1b[1;35m{}\x1b[0m | \x1b[1;31mTGP\x1b[0m: {}W | \x1b[1;36mUV\x1b[0m: {}mV", crate::i18n::t("gpu_mux"), mux_mode, gpu_w, undervolt_mv),
        if let Some(bat) = sysmon_json.get("battery").filter(|b| !b.is_null()) {
            let charge = bat["charge_pct"].as_u64().unwrap_or(0);
            let health_str = bat["health_pct"].as_f64().map(|h| format!("{:.0}%", h)).unwrap_or_else(|| "N/A".into());
            let status_str = bat["status"].as_str().unwrap_or("Unknown");
            let limit_str = if battery_limit < 100 { format!(" (Care: {}%)", battery_limit) } else { String::new() };
            format!("\x1b[1;36mBattery\x1b[0m: {}% [{}] | \x1b[1;36mHealth\x1b[0m: {}{}", charge, status_str, health_str, limit_str)
        } else {
            format!("\x1b[1;36m{}\x1b[0m: {}% {}", crate::i18n::t("battery_care"), battery_limit, crate::i18n::t("limit"))
        },
        format!("\x1b[1;36mRGB\x1b[0m: {}", rgb_mode),
        format!("\x1b[1;36m{}\x1b[0m: {}", crate::i18n::t("conflicts"), if conflict_clean { format!("\x1b[32m{}\x1b[0m", crate::i18n::t("clean")) } else { format!("\x1b[31m{}\x1b[0m", crate::i18n::t("warning")) }),
        "\x1b[40m   \x1b[41m   \x1b[42m   \x1b[43m   \x1b[44m   \x1b[45m   \x1b[46m   \x1b[47m   \x1b[0m".to_string(),
        "\x1b[100m   \x1b[101m   \x1b[102m   \x1b[103m   \x1b[104m   \x1b[105m   \x1b[106m   \x1b[107m   \x1b[0m".to_string(),
    ];

    let ascii_logo = [
        "\x1b[38;2;0;162;255m         .::.         \x1b[0m",
        "\x1b[38;2;20;140;255m       .::::::.       \x1b[0m",
        "\x1b[38;2;40;120;255m     .::::::::::.     \x1b[0m",
        "\x1b[38;2;60;100;255m   .::::::::::::::.   \x1b[0m",
        "\x1b[38;2;80;80;255m  .::::::::::::::::.  \x1b[0m",
        "\x1b[38;2;110;60;255m :::::::::::::::::::: \x1b[0m",
        "\x1b[38;2;140;40;255m :::::::::::::::::::: \x1b[0m",
        "\x1b[38;2;170;20;255m  '::::::::::::::::'  \x1b[0m",
        "\x1b[38;2;200;0;255m   '::::::::::::::'   \x1b[0m",
        "\x1b[38;2;170;20;255m     '::::::::::'     \x1b[0m",
        "\x1b[38;2;140;40;255m       '::::::'       \x1b[0m",
        "\x1b[38;2;0;162;255m         '::'         \x1b[0m",
        "",
        "",
        "",
    ];

    println!();
    let max_lines = ascii_logo.len().max(info_lines.len());
    for i in 0..max_lines {
        let logo_part = ascii_logo.get(i).copied().unwrap_or("                      ");
        let info_part = info_lines.get(i).map(|s| s.as_str()).unwrap_or("");
        println!("{}   {}", logo_part, info_part);
    }
    println!();

    Ok(())
}

async fn execute_input_command(input: &str, conn: &zbus::Connection) -> String {
    let now = chrono::Local::now().format("%H:%M:%S");
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return String::new();
    }

    let fan = FanProxy::new(conn).await.ok();
    let power = PowerProxy::new(conn).await.ok();
    let platform = PlatformProxy::new(conn).await.ok();
    let rgb = RgbProxy::new(conn).await.ok();
    let mux = MuxProxy::new(conn).await.ok();

    match parts[0].to_lowercase().as_str() {
        "fan" => {
            if parts.len() < 2 {
                return format!("[{}] \x1b[33m{}\x1b[0m", now, crate::i18n::t("usage_fan"));
            }
            let sub = parts[1].to_lowercase();
            if let Some(f) = fan {
                if sub == "auto" || sub == "ec" || sub == "max" || sub == "custom" {
                    match f.set_fan_mode(&sub).await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("fan_changed").replace("{}", &sub.to_uppercase())),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("fan_error"), e),
                    }
                } else if let Ok(val) = sub.parse::<u32>() {
                    let rpm = if val <= 100 { val * 60 } else { val };
                    let _ = f.set_fan_mode("custom").await;
                    let r1 = f.set_fan_target(1, rpm).await;
                    let r2 = f.set_fan_target(2, rpm).await;
                    if r1.is_ok() || r2.is_ok() {
                        format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("fan_set").replacen("{}", &val.min(100).to_string(), 1).replacen("{}", &rpm.to_string(), 1))
                    } else {
                        format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("fan_set_failed"))
                    }
                } else {
                    format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("fan_invalid"), sub)
                }
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("fan_no_service"))
            }
        }
        "perf" | "power" => {
            if parts.len() < 2 {
                return format!("[{}] \x1b[33m{}\x1b[0m", now, crate::i18n::t("usage_perf"));
            }
            let sub = parts[1].to_lowercase();
            if let Some(p) = power {
                if sub == "performance" || sub == "perf" {
                    match p.set_power_profile("performance").await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("perf_perf")),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("perf_error"), e),
                    }
                } else if sub == "balanced" || sub == "bal" {
                    match p.set_power_profile("balanced").await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("perf_bal")),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("perf_error"), e),
                    }
                } else if sub == "eco" || sub == "quiet" || sub == "power-saver" || sub == "saver" {
                    match p.set_power_profile("power-saver").await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("perf_eco")),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("perf_error"), e),
                    }
                } else if let (Ok(pl1), Some(pl2)) = (sub.parse::<i32>(), parts.get(2).and_then(|s| s.parse::<i32>().ok())) {
                    match p.set_power_limits(true, pl1, pl2).await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("perf_limits").replacen("{}", &pl1.to_string(), 1).replacen("{}", &pl2.to_string(), 1)),
                        Err(e) => format!("[{}] \x1b[1;31mPower limits error: {}\x1b[0m", now, e),
                    }
                } else {
                    format!("[{}] \x1b[1;31mUnknown power profile: {}\x1b[0m", now, sub)
                }
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("perf_no_service"))
            }
        }
        "bat" | "battery" => {
            if parts.len() < 2 {
                return format!("[{}] \x1b[33m{}\x1b[0m", now, crate::i18n::t("usage_bat"));
            }
            if let Ok(limit) = parts[1].parse::<u32>() {
                if let Some(p) = platform {
                    match p.set_battery_care(limit).await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("bat_set").replacen("{}", &limit.clamp(50, 100).to_string(), 1)),
                        Err(e) => format!("[{}] \x1b[1;31mBattery care error: {}\x1b[0m", now, e),
                    }
                } else {
                    format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("bat_no_service"))
                }
            } else {
                format!("[{}] \x1b[1;31mInvalid battery limit (expected 50-100)\x1b[0m", now)
            }
        }
        "mux" => {
            if parts.len() < 2 {
                if let Some(m) = mux {
                    let info_str = m.get_gpu_info().await.unwrap_or_default();
                    let info_json: Value = serde_json::from_str(&info_str).unwrap_or(Value::Null);
                    let mode = info_json["mode"].as_str().unwrap_or("Unknown").to_string();
                    return format!("[{}] \x1b[36mCurrent GPU MUX Mode: {}\x1b[0m", now, mode);
                }
                return format!("[{}] \x1b[33m{}\x1b[0m", now, crate::i18n::t("usage_mux"));
            }
            let mode = parts[1].to_lowercase();
            if let Some(m) = mux {
                match m.set_gpu_mode(&mode).await {
                    Ok(res) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("mux_set").replacen("{}", &mode, 1).replacen("{}", &res, 1)),
                    Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("mux_error"), e),
                }
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("mux_no_service"))
            }
        }
        "uv" | "undervolt" => {
            if parts.len() < 2 {
                return format!("[{}] \x1b[33mUsage: uv <mv_offset> (e.g. uv -50)\x1b[0m", now);
            }
            if let Ok(mv) = parts[1].parse::<i32>() {
                if let Some(p) = power {
                    match p.set_undervolt(mv).await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("uv_set").replacen("{}", &mv.to_string(), 1)),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("uv_error"), e),
                    }
                } else {
                    format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("perf_no_service"))
                }
            } else {
                format!("[{}] \x1b[1;31mInvalid offset (e.g. -50)\x1b[0m", now)
            }
        }
        "rgb" => {
            if parts.len() < 2 {
                return format!("[{}] \x1b[33mUsage: rgb on | off | red | blue | green | white | hex\x1b[0m", now);
            }
            let color_arg = parts[1].to_lowercase();
            if let Some(r) = rgb {
                if color_arg == "off" {
                    match r.set_global(false, 0, "right").await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("rgb_off")),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("rgb_error"), e),
                    }
                } else if color_arg == "on" {
                    match r.set_global(true, 100, "right").await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("rgb_on")),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("rgb_error"), e),
                    }
                } else {
                    let hex = match color_arg.as_str() {
                        "red" => "FF0000",
                        "green" => "00FF00",
                        "blue" => "0000FF",
                        "white" => "FFFFFF",
                        "yellow" => "FFFF00",
                        "cyan" => "00FFFF",
                        "magenta" | "purple" => "FF00FF",
                        "orange" => "FF8800",
                        other => other.trim_start_matches('#'),
                    };
                    match r.set_color(8, hex).await {
                        Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("rgb_color").replacen("{}", hex, 1)),
                        Err(e) => format!("[{}] \x1b[1;31m{} {}\x1b[0m", now, crate::i18n::t("rgb_error"), e),
                    }
                }
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("rgb_no_service"))
            }
        }
        "clean" => {
            if let Some(p) = platform {
                match p.clean_memory().await {
                    Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("cache_cleared")),
                    Err(e) => format!("[{}] \x1b[1;31mMemory clean error: {}\x1b[0m", now, e),
                }
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("bat_no_service"))
            }
        }
        "diag" | "diagnostics" => {
            if let Some(p) = platform {
                let _ = p.run_wmi_diagnostics().await;
                format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("diag_initiated"))
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("bat_no_service"))
            }
        }
        "triage" => {
            if let Some(p) = platform {
                match p.generate_triage_bundle().await {
                    Ok(path) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("diag_bundle").replacen("{}", &path, 1)),
                    Err(e) => format!("[{}] \x1b[1;31mTriage error: {}\x1b[0m", now, e),
                }
            } else {
                format!("[{}] \x1b[1;31m{}\x1b[0m", now, crate::i18n::t("bat_no_service"))
            }
        }
        "help" => {
            format!("[{}] \x1b[1;33mShortcuts:\x1b[0m fan auto|ec|max|50 | perf perf|bal|eco | mux hybrid|discrete | uv -50 | bat 80 | rgb red|off | exit", now)
        }
        _ => {
            // Fallback to clap subcommand parser
            use clap::Parser;
            use crate::Cli;
            use crate::run_command;

            let args = format!("omen-cli {}", input);
            let args_vec = match shlex::split(&args) {
                Some(v) => v,
                None => return format!("[{}] \x1b[31mInvalid input quotation\x1b[0m", now),
            };

            match Cli::try_parse_from(args_vec) {
                Ok(parsed_cli) => {
                    if let Some(cmd) = &parsed_cli.command {
                        match run_command(cmd, conn).await {
                            Ok(_) => format!("[{}] \x1b[1;32m{}\x1b[0m", now, crate::i18n::t("executed").replacen("{}", input, 1)),
                            Err(e) => format!("[{}] \x1b[1;31mError executing '{}': {}\x1b[0m", now, input, e),
                        }
                    } else {
                        format!("[{}] \x1b[33mType 'help' to list commands\x1b[0m", now)
                    }
                }
                Err(e) => {
                    format!("[{}] \x1b[31mError: {}\x1b[0m", now, e.render().to_string().replace("\n", " "))
                }
            }
        }
    }
}

fn get_os_release_name() -> Option<String> {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if line.starts_with("PRETTY_NAME=") {
                return Some(line.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string());
            }
        }
    }
    None
}
