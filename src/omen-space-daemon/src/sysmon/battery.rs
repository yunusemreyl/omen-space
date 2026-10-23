use omen_types::{BatteryStatus, BatteryTelemetry};
use std::fs;
use std::path::{Path, PathBuf};

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_u64(path: &Path) -> Option<u64> {
    read_trimmed(path).and_then(|s| s.parse::<u64>().ok())
}

fn read_u32(path: &Path) -> Option<u32> {
    read_trimmed(path).and_then(|s| s.parse::<u32>().ok())
}

fn parse_battery_status(s: &str) -> BatteryStatus {
    match s.trim().to_lowercase().as_str() {
        "charging" => BatteryStatus::Charging,
        "discharging" => BatteryStatus::Discharging,
        "not charging" | "not_charging" => BatteryStatus::NotCharging,
        "full" => BatteryStatus::Full,
        _ => BatteryStatus::Unknown,
    }
}

/// Check if any mains / AC / USB-PD power supply reports `online == 1`.
pub fn probe_ac_online(power_supply_dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(power_supply_dir) else {
        return false;
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Exclude peripheral devices (e.g. wireless mouse dongle)
        if let Some(scope) = read_trimmed(&path.join("scope")) {
            if scope.eq_ignore_ascii_case("device") {
                continue;
            }
        }

        let psy_type = read_trimmed(&path.join("type")).unwrap_or_default();
        let name = entry.file_name().to_string_lossy().to_string();

        let is_ac = psy_type.eq_ignore_ascii_case("mains")
            || psy_type.eq_ignore_ascii_case("usb")
            || name.starts_with("AC")
            || name.starts_with("ADP")
            || name.starts_with("ucsi");

        if is_ac {
            if let Some(online) = read_trimmed(&path.join("online")) {
                if online == "1" {
                    return true;
                }
            }
        }
    }

    false
}

/// Find the primary internal system battery path.
pub fn find_battery_path(power_supply_dir: &Path) -> Option<PathBuf> {
    let Ok(entries) = fs::read_dir(power_supply_dir) else {
        return None;
    };

    let mut candidates = Vec::new();

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Must be battery type
        let Some(psy_type) = read_trimmed(&path.join("type")) else {
            continue;
        };
        if !psy_type.eq_ignore_ascii_case("battery") {
            continue;
        }

        // Exclude peripheral devices (e.g. hidpp_battery_0)
        if let Some(scope) = read_trimmed(&path.join("scope")) {
            if scope.eq_ignore_ascii_case("device") {
                continue;
            }
        }

        // Must be present if 'present' attribute exists
        if let Some(present) = read_trimmed(&path.join("present")) {
            if present == "0" {
                continue;
            }
        }

        let name = entry.file_name().to_string_lossy().to_string();
        candidates.push((name, path));
    }

    if candidates.is_empty() {
        return None;
    }

    // Prefer BAT0, then BAT1, then lexical order
    candidates.sort_by(|a, b| {
        if a.0 == "BAT0" {
            std::cmp::Ordering::Less
        } else if b.0 == "BAT0" {
            std::cmp::Ordering::Greater
        } else if a.0 == "BAT1" {
            std::cmp::Ordering::Less
        } else if b.0 == "BAT1" {
            std::cmp::Ordering::Greater
        } else {
            a.0.cmp(&b.0)
        }
    });

    Some(candidates.remove(0).1)
}

/// Pure battery inspection against a sysfs root (e.g. `/sys`).
pub fn probe_battery(sysfs_root: &Path) -> Option<BatteryTelemetry> {
    let psy_dir = sysfs_root.join("class/power_supply");
    if !psy_dir.exists() {
        return None;
    }

    let bat_path = find_battery_path(&psy_dir)?;
    let name = bat_path.file_name()?.to_string_lossy().to_string();

    let status_str = read_trimmed(&bat_path.join("status")).unwrap_or_else(|| "Unknown".to_string());
    let status = parse_battery_status(&status_str);

    let ac_online = probe_ac_online(&psy_dir);

    // Charge percentage
    let mut charge_pct = read_u32(&bat_path.join("capacity")).map(|c| c.min(100) as u8);

    // Voltage
    let voltage_now_uv = read_u64(&bat_path.join("voltage_now"));
    let voltage_min_design_uv = read_u64(&bat_path.join("voltage_min_design"));
    let voltage_v = voltage_now_uv.map(|v| v as f64 / 1_000_000.0);

    // Energy (µWh) vs Charge (µAh)
    let energy_now_uwh = read_u64(&bat_path.join("energy_now"));
    let energy_full_uwh = read_u64(&bat_path.join("energy_full"));
    let energy_design_uwh = read_u64(&bat_path.join("energy_full_design"));

    let charge_now_uah = read_u64(&bat_path.join("charge_now"));
    let charge_full_uah = read_u64(&bat_path.join("charge_full"));
    let charge_design_uah = read_u64(&bat_path.join("charge_full_design"));

    let (energy_now_wh, energy_full_wh, energy_design_wh, health_pct) = if let Some(e_now) = energy_now_uwh {
        let e_full = energy_full_uwh;
        let e_des = energy_design_uwh;

        let health = match (e_full, e_des) {
            (Some(f), Some(d)) if d > 0 => Some(((f as f64 / d as f64) * 100.0).clamp(0.0, 100.0)),
            _ => None,
        };

        (
            Some(e_now as f64 / 1_000_000.0),
            e_full.map(|f| f as f64 / 1_000_000.0),
            e_des.map(|d| d as f64 / 1_000_000.0),
            health,
        )
    } else if let Some(c_now) = charge_now_uah {
        let v_ref = voltage_min_design_uv.or(voltage_now_uv).unwrap_or(0);
        let c_full = charge_full_uah;
        let c_des = charge_design_uah;

        let health = match (c_full, c_des) {
            (Some(f), Some(d)) if d > 0 => Some(((f as f64 / d as f64) * 100.0).clamp(0.0, 100.0)),
            _ => None,
        };

        let to_wh = |uah: u64| -> Option<f64> {
            if v_ref > 0 {
                Some((uah as f64 * v_ref as f64) / 1_000_000_000_000.0)
            } else {
                None
            }
        };

        (to_wh(c_now), c_full.and_then(to_wh), c_des.and_then(to_wh), health)
    } else {
        (None, None, None, None)
    };

    if charge_pct.is_none() {
        if let (Some(now), Some(full)) = (energy_now_wh, energy_full_wh) {
            if full > 0.0 {
                charge_pct = Some(((now / full) * 100.0).round().clamp(0.0, 100.0) as u8);
            }
        }
    }

    // Instantaneous Power Draw
    let power_w = if let Some(p_uw) = read_u64(&bat_path.join("power_now")) {
        p_uw as f64 / 1_000_000.0
    } else if let (Some(cur_ua), Some(vol_uv)) = (read_u64(&bat_path.join("current_now")), voltage_now_uv) {
        (cur_ua as f64 * vol_uv as f64) / 1_000_000_000_000.0
    } else {
        0.0
    };

    let cycle_count = read_u32(&bat_path.join("cycle_count"));

    let charge_limit_pct = read_u32(&bat_path.join("charge_control_end_threshold"))
        .filter(|&l| (50..=100).contains(&l))
        .map(|l| l as u8);

    Some(BatteryTelemetry {
        name,
        status,
        charge_pct: charge_pct.unwrap_or(0),
        health_pct,
        energy_now_wh,
        energy_full_wh,
        energy_design_wh,
        power_w,
        voltage_v,
        cycle_count,
        ac_online,
        charge_limit_pct,
    })
}

/// Instantaneous battery telemetry from the live system (`/sys`).
pub fn current_battery() -> Option<BatteryTelemetry> {
    probe_battery(Path::new("/sys"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("omen-space-battery-test-{}", tag));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("class/power_supply")).unwrap();
            Tree { root }
        }

        fn sys(&self) -> &Path {
            &self.root
        }

        fn write(&self, rel: &str, content: &str) {
            let path = self.root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut file = File::create(path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn test_energy_reporting_battery_hp_omen() {
        let t = Tree::new("hp-omen");
        t.write("class/power_supply/BAT0/type", "Battery\n");
        t.write("class/power_supply/BAT0/status", "Not charging\n");
        t.write("class/power_supply/BAT0/capacity", "79\n");
        t.write("class/power_supply/BAT0/energy_now", "44136000\n");
        t.write("class/power_supply/BAT0/energy_full", "55517000\n");
        t.write("class/power_supply/BAT0/energy_full_design", "55517000\n");
        t.write("class/power_supply/BAT0/power_now", "0\n");
        t.write("class/power_supply/BAT0/voltage_now", "16463000\n");
        t.write("class/power_supply/BAT0/voltage_min_design", "15400000\n");
        t.write("class/power_supply/BAT0/cycle_count", "44\n");
        t.write("class/power_supply/BAT0/present", "1\n");

        t.write("class/power_supply/ACAD/type", "Mains\n");
        t.write("class/power_supply/ACAD/online", "1\n");

        let telem = probe_battery(t.sys()).expect("Should discover BAT0");
        assert_eq!(telem.name, "BAT0");
        assert_eq!(telem.status, BatteryStatus::NotCharging);
        assert_eq!(telem.charge_pct, 79);
        assert_eq!(telem.cycle_count, Some(44));
        assert!(telem.ac_online);

        let health = telem.health_pct.unwrap();
        assert!((health - 100.0).abs() < 0.01);

        let energy_now = telem.energy_now_wh.unwrap();
        assert!((energy_now - 44.136).abs() < 0.001);

        let energy_full = telem.energy_full_wh.unwrap();
        assert!((energy_full - 55.517).abs() < 0.001);
    }

    #[test]
    fn test_charge_reporting_battery_wear_and_power() {
        let t = Tree::new("charge-wear");
        t.write("class/power_supply/BAT0/type", "Battery\n");
        t.write("class/power_supply/BAT0/status", "Discharging\n");
        t.write("class/power_supply/BAT0/capacity", "50\n");
        t.write("class/power_supply/BAT0/charge_now", "2000000\n");
        t.write("class/power_supply/BAT0/charge_full", "4000000\n");
        t.write("class/power_supply/BAT0/charge_full_design", "5000000\n"); // 80% health
        t.write("class/power_supply/BAT0/current_now", "1500000\n"); // 1.5A
        t.write("class/power_supply/BAT0/voltage_now", "12000000\n"); // 12.0V -> 18.0W
        t.write("class/power_supply/BAT0/voltage_min_design", "11400000\n");
        t.write("class/power_supply/BAT0/cycle_count", "120\n");
        t.write("class/power_supply/BAT0/charge_control_end_threshold", "80\n");

        let telem = probe_battery(t.sys()).expect("Should discover BAT0");
        assert_eq!(telem.status, BatteryStatus::Discharging);
        assert_eq!(telem.charge_pct, 50);
        assert_eq!(telem.cycle_count, Some(120));
        assert_eq!(telem.charge_limit_pct, Some(80));

        let health = telem.health_pct.unwrap();
        assert!((health - 80.0).abs() < 0.01);

        assert!((telem.power_w - 18.0).abs() < 0.01);
        assert_eq!(telem.ac_online, false);
    }

    #[test]
    fn test_exclude_device_scope_peripheral() {
        let t = Tree::new("exclude-device");
        t.write("class/power_supply/hidpp_battery_0/type", "Battery\n");
        t.write("class/power_supply/hidpp_battery_0/scope", "Device\n");
        t.write("class/power_supply/hidpp_battery_0/capacity", "90\n");

        assert!(probe_battery(t.sys()).is_none());
    }

    #[test]
    fn test_desktop_system_without_battery() {
        let t = Tree::new("desktop");
        t.write("class/power_supply/ACAD/type", "Mains\n");
        t.write("class/power_supply/ACAD/online", "1\n");

        assert!(probe_battery(t.sys()).is_none());
    }

    /// Prints what this very host machine's live battery reports. Run with:
    /// `cargo test -p omen-space-daemon host_battery_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn host_battery_smoke() {
        let bat = current_battery();
        println!("Live battery on host: {bat:#?}");
        if let Some(b) = bat {
            assert!(b.charge_pct <= 100);
            assert_eq!(b.name, "BAT0");
        }
    }
}
