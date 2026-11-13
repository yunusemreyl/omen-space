/// Undervolt service - matches Python intel_undervolt.py feature-for-feature.
///
/// D-Bus interface: com.yyl.hpmanager.undervolt (backward compat) +
///                  org.hp.omen.Undervolt (new canonical name)
///
/// Methods exposed:
///   SetOffset(plane: s, offset_mv: i) -> resp: s
///   GetState()  -> j: s   (offsets per plane + TCC + availability)
///   SetTccOffset(val: i)  -> resp: s
///   ReadOffsets()         -> j: s   (live read from MSR)
///   Ping()                -> resp: s
///
/// Intel MSR 0x150 layout matches intel_undervolt.py exactly:
///   pack_offset(plane, offset) = (1<<63) | (plane<<40) | (1<<36) | (1<<32) | offset_bits
///   offset_bits = 0xFFE00000 & ((x & 0xFFF) << 21)  where x = round(mV * 1.024)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;
use log::{info, warn};

const CONFIG_PATH: &str = "/etc/omen-space/undervolt.json";

// Intel MSR addresses matching intel_undervolt.py ADDRESSES
const MSR_VOLTAGE_OFFSETS: u64 = 0x150;
const MSR_TEMPERATURE: u64 = 0x1a2;

// Voltage plane indices matching Python PLANES dict
const PLANE_CORE: u32    = 0;
const PLANE_GPU: u32     = 1;
const PLANE_CACHE: u32   = 2;
const PLANE_UNCORE: u32  = 3;
const PLANE_ANALOGIO: u32 = 4;

fn plane_index(plane: &str) -> Option<u32> {
    match plane.to_lowercase().as_str() {
        "core"     => Some(PLANE_CORE),
        "gpu"      => Some(PLANE_GPU),
        "cache"    => Some(PLANE_CACHE),
        "uncore"   => Some(PLANE_UNCORE),
        "analogio" => Some(PLANE_ANALOGIO),
        _ => None,
    }
}

// ── MSR math — direct port of intel_undervolt.py ──────────────────────────────

/// convert_offset(mV) → offset bits
/// Python: convert_rounded_offset(round(mV * 1.024))
fn convert_offset(mv: i32) -> u64 {
    let rounded = (mv as f64 * 1.024).round() as i32;
    // convert_rounded_offset
    0xFFE00000u64 & (((rounded as u64) & 0xFFF) << 21)
}

/// pack_offset(plane, offset_bits) for write
/// Python: (1<<63) | (plane<<40) | (1<<36) | (1<<32) | offset
fn pack_offset_write(plane: u32, offset_bits: u64) -> u64 {
    (1u64 << 63) | ((plane as u64) << 40) | (1u64 << 36) | (1u64 << 32) | offset_bits
}

/// pack_offset(plane) for read (no offset bits, no write flag)
fn pack_offset_read(plane: u32) -> u64 {
    (1u64 << 63) | ((plane as u64) << 40) | (1u64 << 36)
}

/// unconvert_offset: offset bits → mV (approximate)
fn unconvert_offset(bits: u64) -> f64 {
    let x = (bits >> 21) as i32;
    let rounded = if x <= 1024 { x } else { -(2048 - x) };
    rounded as f64 / 1.024
}

/// unpack_offset: from MSR read response → mV
fn unpack_offset(msr_response: u64) -> f64 {
    let plane_index = msr_response >> 40;
    let offset_bits = msr_response ^ (plane_index << 40);
    unconvert_offset(offset_bits)
}

// ── MSR I/O ───────────────────────────────────────────────────────────────────

fn msr_path(cpu: usize) -> String {
    format!("/dev/cpu/{}/msr", cpu)
}

fn count_cpus() -> usize {
    static CPU_COUNT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CPU_COUNT.get_or_init(|| {
        (0..256).filter(|&i| Path::new(&msr_path(i)).exists()).count().max(1)
    })
}

fn write_msr(val: u64, addr: u64) -> bool {
    let cpu_count = count_cpus();
    let mut ok = false;
    for cpu in 0..cpu_count {
        let path = msr_path(cpu);
        if let Ok(mut f) = OpenOptions::new().write(true).open(&path) {
            if f.seek(SeekFrom::Start(addr)).is_ok() {
                let bytes = val.to_le_bytes();
                if f.write_all(&bytes).is_ok() { ok = true; }
            }
        }
    }
    ok
}

fn read_msr(addr: u64, cpu: usize) -> Option<u64> {
    let path = msr_path(cpu);
    let mut f = File::open(&path).ok()?;
    f.seek(SeekFrom::Start(addr)).ok()?;
    let mut buf = [0u8; 8];
    f.read_exact(&mut buf).ok()?;
    Some(u64::from_le_bytes(buf))
}

fn ensure_msr_module() {
    if !Path::new("/dev/cpu/0/msr").exists() {
        let _ = std::process::Command::new("modprobe").arg("msr").output();
    }
}

// ── Config ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UndervoltConfig {
    offsets: HashMap<String, i32>,
    tcc_offset: i32,
    pending_confirmation: bool,
}

impl Default for UndervoltConfig {
    fn default() -> Self {
        let mut offsets = HashMap::new();
        for p in ["core", "gpu", "cache", "uncore", "analogio"] {
            offsets.insert(p.to_string(), 0);
        }
        Self { offsets, tcc_offset: 0, pending_confirmation: false }
    }
}

impl UndervoltConfig {
    fn load() -> Self {
        if let Ok(data) = std::fs::read_to_string(CONFIG_PATH) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }
    fn save(&self) {
        if let Some(dir) = Path::new(CONFIG_PATH).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_PATH, json);
        }
    }
}

// ── Service ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct UndervoltService {
    config: Arc<Mutex<UndervoltConfig>>,
    available: bool,
    is_amd: bool,
    voltage_locked: bool,
}

fn is_cpu_voltage_locked(cpu_spec: &str) -> bool {
    let cpu_upper = cpu_spec.to_uppercase();
    if cpu_upper.contains("AMD") {
        return false;
    }
    // Intel Core Ultra (Meteor Lake / Arrow Lake / Lunar Lake, including HX SKUs)
    // does not support legacy FIVR MSR 0x150 voltage offsets (#338).
    if cpu_upper.contains("CORE ULTRA") || cpu_upper.contains("ULTRA ") {
        return true;
    }
    let is_unlocked_hx = cpu_upper.contains("HX") || cpu_upper.contains("HK") || cpu_upper.contains("-K ");
    let is_locked_gen = cpu_upper.contains("12TH GEN")
        || cpu_upper.contains("13TH GEN")
        || cpu_upper.contains("14TH GEN")
        || cpu_upper.contains("I5-12")
        || cpu_upper.contains("I7-12")
        || cpu_upper.contains("I9-12")
        || cpu_upper.contains("I5-13")
        || cpu_upper.contains("I7-13")
        || cpu_upper.contains("I9-13")
        || cpu_upper.contains("I5-14")
        || cpu_upper.contains("I7-14")
        || cpu_upper.contains("I9-14");
    is_locked_gen && !is_unlocked_hx
}

const INTEL_TCC_SYSFS_PATH: &str = "/sys/bus/pci/devices/0000:00:04.0/tcc_offset_degree_celsius";

/// Read `(tj_max, hw_tcc_offset, tcc_locked)` from Intel MSR 0x1a2 (`MSR_IA32_TEMPERATURE_TARGET`)
/// and/or the kernel's `processor_thermal_device_pci` sysfs node.
/// MSR 0x1a2 layout on modern Intel:
///   - Bit 31: Locked (1 = locked by BIOS/firmware until reset)
///   - Bits 29:24: TCC Activation Offset (6-bit unsigned degree offset)
///   - Bits 23:16: Temperature Target / TjMax (8-bit °C, e.g. 100 or 105)
fn read_intel_tcc_hw() -> (i32, Option<i32>, bool) {
    let mut tj_max = 100;
    let mut hw_offset = None;
    let mut locked = false;

    if let Some(raw) = read_msr(MSR_TEMPERATURE, 0) {
        locked = ((raw >> 31) & 1) != 0;
        let tj = ((raw >> 16) & 0xFF) as i32;
        if (70..=130).contains(&tj) {
            tj_max = tj;
        }
        let off = ((raw >> 24) & 0x3F) as i32;
        hw_offset = Some(off);
    }

    if let Ok(s) = std::fs::read_to_string(INTEL_TCC_SYSFS_PATH) {
        if let Ok(sysfs_off) = s.trim().parse::<i32>() {
            if sysfs_off >= 0 {
                hw_offset = Some(sysfs_off);
            }
        }
    }

    (tj_max, hw_offset, locked)
}

fn has_ryzenadj() -> bool {
    const COMMON_PATHS: &[&str] = &[
        "/usr/bin/ryzenadj",
        "/usr/local/bin/ryzenadj",
        "/bin/ryzenadj",
        "/usr/sbin/ryzenadj",
        "/usr/local/sbin/ryzenadj",
    ];

    if COMMON_PATHS.iter().any(|path| Path::new(path).is_file()) {
        return true;
    }

    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("ryzenadj"))
                .any(|path| path.is_file())
        })
        .unwrap_or(false)
}


impl UndervoltService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let specs = crate::sysmon::get_hardware_specs();
        let is_amd = specs.cpu_spec.to_uppercase().contains("AMD");
        let voltage_locked = is_cpu_voltage_locked(&specs.cpu_spec);
        ensure_msr_module();
        
        let available = if is_amd {
            has_ryzenadj()
        } else {
            Path::new("/dev/cpu/0/msr").exists() || Path::new(INTEL_TCC_SYSFS_PATH).exists()
        };
        
        let config = UndervoltConfig::load();

        let svc = Self {
            config: Arc::new(Mutex::new(config)),
            available,
            is_amd,
            voltage_locked,
        };

        if available {
            svc.check_startup_recovery().await;
            svc.apply_all_offsets().await;
            svc.restore_saved_tcc_offset().await;
        } else {
            if is_amd {
                warn!("Undervolt: ryzenadj is not installed. AMD tuning unavailable.");
            } else {
                warn!("Undervolt: /dev/cpu/0/msr not available (is msr module loaded?)");
            }
        }

        Ok(svc)
    }

    async fn apply_all_offsets(&self) {
        if self.voltage_locked {
            info!("Undervolt: CPU voltage control is hardware-locked on this SKU; skipping MSR 0x150 startup restore (#313).");
            return;
        }
        let cfg = self.config.lock().await.clone();
        for (plane, &mv) in &cfg.offsets {
            if mv == 0 { continue; }
            if self.is_amd {
                if plane == "core" {
                    let count = mv.clamp(-30, 30);
                    let _ = std::process::Command::new("ryzenadj").arg(format!("--set-coall={}", count)).output();
                    info!("Undervolt: AMD Curve Optimizer restored to {}", count);
                }
            } else {
                if let Some(idx) = plane_index(plane) {
                    let bits = convert_offset(mv);
                    let val = pack_offset_write(idx, bits);
                    if write_msr(val, MSR_VOLTAGE_OFFSETS) {
                        info!("Undervolt: restored {} = {}mV", plane, mv);
                    } else {
                        warn!("Undervolt: failed to restore {} = {}mV", plane, mv);
                    }
                }
            }
        }
    }

    /// The TCC offset lives in MSR 0x1a2, which firmware resets on every boot, so a saved
    /// offset was silently lost after a reboot while the config still showed it. Re-apply it
    /// at startup. Only a non-zero saved value is restored: 0 is also the "never set" default,
    /// and writing it could lower a firmware-provided offset.
    async fn restore_saved_tcc_offset(&self) {
        let saved = self.config.lock().await.tcc_offset;
        if !Self::should_restore_tcc(saved, if self.is_amd { None } else { read_intel_tcc_hw().1 }) {
            return;
        }
        let result = self.set_tcc_offset(saved).await;
        if result == "OK" {
            info!("Undervolt: restored saved TCC offset {}°C", saved);
        } else {
            warn!("Undervolt: could not restore saved TCC offset {}°C: {}", saved, result);
        }
    }

    pub(crate) fn should_restore_tcc(saved: i32, hw_offset: Option<i32>) -> bool {
        saved > 0 && hw_offset != Some(saved)
    }

    async fn check_startup_recovery(&self) {
        let mut cfg = self.config.lock().await;
        if cfg.pending_confirmation {
            warn!("Undervolt: System crashed or rebooted without confirming the undervolt settings!");
            warn!("Undervolt: TuningStartupRecoveryGuard triggered. Resetting all offsets to 0.");
            for mv in cfg.offsets.values_mut() {
                *mv = 0;
            }
            cfg.pending_confirmation = false;
            cfg.save();
        }
    }

    fn detect_external_controller(&self) -> Option<String> {
        let procs = ["intel-undervolt", "throttled", "thermald"];
        for p in procs {
            if let Ok(output) = std::process::Command::new("pgrep").arg("-x").arg(p).output() {
                if output.status.success() {
                    return Some(if p == "thermald" { "thermald (may override TCC)" } else { p }.to_string());
                }
            }
        }
        None
    }
}

// ── D-Bus interface ────────────────────────────────────────────────────────────

#[interface(name = "org.hp.omen.Undervolt")]
impl UndervoltService {
    /// SetOffset(plane, offset_mv) — write voltage offset to Intel MSR 0x150.
    /// Mirrors Python intel_undervolt.set_offset().
    async fn set_offset(&self, plane: String, offset_mv: i32) -> String {
        let mv = if self.is_amd { offset_mv.clamp(-30, 30) } else { offset_mv.clamp(-250, 0) };
        let Some(idx) = plane_index(&plane) else {
            warn!("SetOffset: unknown plane '{}'", plane);
            return "FAIL".to_string();
        };

        if !self.available {
            warn!("SetOffset: MSR or ryzenadj not available");
            return "FAIL".to_string();
        }

        if self.voltage_locked {
            if mv == 0 {
                return "OK".to_string();
            }
            warn!("SetOffset: rejected plane='{}' val={}mV — CPU voltage offsets are hardware-locked on this processor (#313)", plane, mv);
            return "FAIL: CPU voltage offset is locked by hardware on this processor".to_string();
        }

        // Avoid touching MSR 0x150 when asked to set 0 mV and the saved offset for this plane is already 0 (#313).
        if mv == 0 {
            let prev = self
                .config
                .lock()
                .await
                .offsets
                .get(&plane.to_lowercase())
                .copied()
                .unwrap_or(0);
            if prev == 0 {
                return "OK".to_string();
            }
        }

        if self.is_amd {
            if plane == "core" {
                let _ = std::process::Command::new("ryzenadj").arg(format!("--set-coall={}", mv)).output();
            }
        } else {
            let bits = convert_offset(mv);
            let write_val = pack_offset_write(idx, bits);

            if !write_msr(write_val, MSR_VOLTAGE_OFFSETS) {
                warn!("SetOffset: MSR write failed for plane '{}'", plane);
                return "FAIL".to_string();
            }

            // Verify: read back and confirm
            let read_val = pack_offset_read(idx);
            if write_msr(read_val, MSR_VOLTAGE_OFFSETS) {
                if let Some(readback) = read_msr(MSR_VOLTAGE_OFFSETS, 0) {
                    let read_mv = unpack_offset(readback);
                    let want_mv = unconvert_offset(bits);
                    if (read_mv - want_mv).abs() > 2.0 {
                        warn!("SetOffset: verify failed: wrote {}mV, read {}mV", want_mv, read_mv);
                        return "FAIL: MSR voltage offset readback verification failed".to_string();
                    }
                }
            }
        }

        // Persist with pending confirmation (TuningStartupRecoveryGuard)
        {
            let mut cfg = self.config.lock().await;
            cfg.offsets.insert(plane.to_lowercase(), mv);
            cfg.pending_confirmation = true;
            cfg.save();
        }

        info!("SetOffset: plane='{}' val={}", plane, mv);
        "OK".to_string()
    }

    /// SetTccOffset(val) — write Intel TCC (thermal throttle) offset to native sysfs or MSR 0x1a2.
    /// Mirrors Python SetTccOffset(). Range 0–15.
    async fn set_tcc_offset(&self, val: i32) -> String {
        let val = val.clamp(0, 15);

        if !self.available {
            return "FAIL: TCC offset interface unavailable".to_string();
        }

        if self.is_amd {
            let max_temp = 100 - val;
            let ok = std::process::Command::new("ryzenadj")
                .arg(format!("--tctl-temp={}", max_temp))
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                return "FAIL: ryzenadj failed to set tctl-temp".to_string();
            }
        } else {
            let (_tj_max, hw_off, locked) = read_intel_tcc_hw();
            if locked {
                if hw_off == Some(val) {
                    return "OK".to_string();
                }
                warn!(
                    "SetTccOffset({}): MSR 0x1a2 bit 31 is locked by firmware (current hardware offset: {:?}°C)",
                    val, hw_off
                );
                return format!(
                    "FAIL: TCC offset is locked by firmware at {}°C",
                    hw_off.unwrap_or(0)
                );
            }

            // 1. Prefer the kernel's native processor_thermal_device_pci sysfs attribute when available (#313).
            //    If the native sysfs attribute exists and rejects the write, fail cleanly instead of falling
            //    back to raw MSR 0x1a2 writes that may have a different register layout (#338).
            if Path::new(INTEL_TCC_SYSFS_PATH).exists() {
                if std::fs::write(INTEL_TCC_SYSFS_PATH, format!("{}\n", val)).is_ok() {
                    let (_, new_hw, _) = read_intel_tcc_hw();
                    if new_hw == Some(val) {
                        {
                            let mut cfg = self.config.lock().await;
                            cfg.tcc_offset = val;
                            cfg.save();
                        }
                        info!("SetTccOffset (sysfs): {}", val);
                        return "OK".to_string();
                    }
                }
                warn!(
                    "SetTccOffset({}): native sysfs {} rejected write or readback mismatch",
                    val, INTEL_TCC_SYSFS_PATH
                );
                return "FAIL: Native sysfs TCC offset write rejected".to_string();
            }

            // 2. Fallback to MSR 0x1a2 read-modify-write (bits 29:24, preserving TjMax in bits 23:16) with readback verification
            {
                let mut write_ok = true;
                let cpu_count = count_cpus();
                for cpu in 0..cpu_count {
                    let path = msr_path(cpu);
                    if let Ok(mut f) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
                        use std::io::{Seek, SeekFrom, Read, Write};
                        if f.seek(SeekFrom::Start(MSR_TEMPERATURE)).is_ok() {
                            let mut bytes = [0u8; 8];
                            if f.read_exact(&mut bytes).is_ok() {
                                let cur_val = u64::from_le_bytes(bytes);
                                // Clear bits 29:24 (mask 0x3F000000), then set them to the new offset
                                let new_val = (cur_val & !0x3F000000u64) | ((val as u64 & 0x3F) << 24);
                                if f.seek(SeekFrom::Start(MSR_TEMPERATURE)).is_ok() {
                                    if f.write_all(&new_val.to_le_bytes()).is_err() {
                                        warn!("SetTccOffset: MSR 0x1a2 write failed on CPU {}", cpu);
                                        write_ok = false;
                                    }
                                } else {
                                    write_ok = false;
                                }
                            } else {
                                write_ok = false;
                            }
                        } else {
                            write_ok = false;
                        }
                    } else {
                        write_ok = false;
                    }
                }
                let (_, verify_off, _) = read_intel_tcc_hw();
                if !write_ok || verify_off != Some(val) {
                    warn!(
                        "SetTccOffset({}): write or readback verification failed (readback={:?})",
                        val, verify_off
                    );
                    return "FAIL: Failed to apply or verify TCC offset in hardware".to_string();
                }
            }
        }

        {
            let mut cfg = self.config.lock().await;
            cfg.tcc_offset = val;
            cfg.save();
        }
        info!("SetTccOffset: {}", val);
        "OK".to_string()
    }

    /// GetState — returns persisted offsets and live MSR reads if available.
    async fn get_state(&self) -> String {
        let cfg = self.config.lock().await.clone();
        let external = self.detect_external_controller();
        let mut warning = None;
        if let Some(ref ext) = external {
            warning = Some(format!("External controller detected: {}. This may conflict with Omen Space.", ext));
        }
        if self.is_amd && !self.available {
            warning = Some("RyzenAdj is not installed. Please install ryzenadj to use AMD Undervolting & Power Limits.".to_string());
        }

        let (tj_max, hw_tcc_offset, tcc_locked) = if self.is_amd {
            (100, None, false)
        } else {
            read_intel_tcc_hw()
        };
        let effective_tcc = hw_tcc_offset.unwrap_or(cfg.tcc_offset);

        let json = serde_json::json!({
            "available": self.available,
            "is_amd": self.is_amd,
            "voltage_locked": self.voltage_locked,
            "offsets": cfg.offsets,
            "tcc_offset": effective_tcc,
            "saved_tcc_offset": cfg.tcc_offset,
            "hw_tcc_offset": hw_tcc_offset,
            "tcc_locked": tcc_locked,
            "tj_max": tj_max,
            "external_controller": external,
            "warning": warning,
        });
        json.to_string()
    }

    /// ReadOffsets — live read of all plane offsets from MSR 0x150.
    async fn read_offsets(&self) -> String {
        if !self.available {
            return serde_json::json!({ "error": "MSR/Ryzenadj not available" }).to_string();
        }
        let mut result = serde_json::Map::new();
        if self.is_amd {
            let cfg = self.config.lock().await.clone();
            for (plane, &val) in &cfg.offsets {
                result.insert(plane.to_string(), (val as f64).into());
            }
            result.insert("tcc_target_c".into(), (100u64 - cfg.tcc_offset as u64).into());
        } else {
            if !self.voltage_locked {
                for (plane, idx) in &[("core", PLANE_CORE), ("gpu", PLANE_GPU),
                                        ("cache", PLANE_CACHE), ("uncore", PLANE_UNCORE)] {
                    let read_req = pack_offset_read(*idx);
                    if write_msr(read_req, MSR_VOLTAGE_OFFSETS) {
                        if let Some(resp) = read_msr(MSR_VOLTAGE_OFFSETS, 0) {
                            let mv = unpack_offset(resp);
                            result.insert(plane.to_string(), mv.into());
                        }
                    }
                }
            }
            let (tj_max, hw_off, locked) = read_intel_tcc_hw();
            let off = hw_off.unwrap_or(0);
            result.insert("tj_max_c".into(), (tj_max as i64).into());
            result.insert("tcc_offset_c".into(), (off as i64).into());
            result.insert("tcc_locked".into(), locked.into());
            result.insert("tcc_target_c".into(), ((tj_max - off).max(0) as u64).into());
        }
        serde_json::Value::Object(result).to_string()
    }

    async fn ping(&self) -> String {
        "OK".to_string()
    }

    /// Confirm the undervolt settings so they are not reset on next boot
    async fn confirm(&self) -> String {
        let mut cfg = self.config.lock().await;
        if cfg.pending_confirmation {
            cfg.pending_confirmation = false;
            cfg.save();
            info!("Undervolt: Settings confirmed by user. Recovery Guard disarmed.");
        }
        "OK".to_string()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn saved_tcc_offset_is_restored_only_when_set_and_different() {
        use super::UndervoltService as U;
        assert!(U::should_restore_tcc(15, Some(3)), "boot reset to firmware default must be restored");
        assert!(U::should_restore_tcc(15, None));
        assert!(!U::should_restore_tcc(15, Some(15)), "already applied");
        assert!(!U::should_restore_tcc(0, Some(3)), "0 is the never-set default; keep firmware offset");
    }

    use super::is_cpu_voltage_locked;

    #[test]
    fn core_ultra_hx_is_voltage_locked() {
        assert!(is_cpu_voltage_locked("Intel(R) Core(TM) Ultra 9 275HX"));
        assert!(is_cpu_voltage_locked("Intel Core Ultra 7 155H"));
        assert!(is_cpu_voltage_locked("13th Gen Intel(R) Core(TM) i7-13700H"));
        assert!(!is_cpu_voltage_locked("14th Gen Intel(R) Core(TM) i7-14700HX"));
        assert!(!is_cpu_voltage_locked("AMD Ryzen 9 8940HX"));
    }
}
