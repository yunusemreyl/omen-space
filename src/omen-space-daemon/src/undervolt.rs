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
        ensure_msr_module();
        
        let available = if is_amd {
            has_ryzenadj()
        } else {
            Path::new("/dev/cpu/0/msr").exists()
        };
        
        let config = UndervoltConfig::load();

        let svc = Self {
            config: Arc::new(Mutex::new(config)),
            available,
            is_amd,
        };

        if available {
            svc.check_startup_recovery().await;
            svc.apply_all_offsets().await;
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

    /// SetTccOffset(val) — write Intel TCC (thermal throttle) offset to MSR 0x1a2.
    /// Mirrors Python SetTccOffset(). Range 0–15.
    async fn set_tcc_offset(&self, val: i32) -> String {
        let val = val.clamp(0, 15);

        if self.available {
            if self.is_amd {
                let max_temp = 100 - val;
                let _ = std::process::Command::new("ryzenadj").arg(format!("--tctl-temp={}", max_temp)).output();
            } else {
                // FIX #7: Proper read-modify-write for MSR 0x1a2 (Temperature Target).
                // We must preserve Tj_max (bits 15:8) and reserved bits. A blind write
                // sets Tj_max to 0, which the CPU silicon rejects with a #GP fault.
                // On Comet Lake (and most modern Intel), TCC Activation Offset is bits 27:24.
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
                                // Clear bits 27:24 (mask 0x0F000000), then set them to the new offset
                                let new_val = (cur_val & !0x0F000000u64) | ((val as u64 & 0x0F) << 24);
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
                if !write_ok {
                    warn!("SetTccOffset: MSR 0x1a2 write failed");
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

        let json = serde_json::json!({
            "available": self.available,
            "is_amd": self.is_amd,
            "offsets": cfg.offsets,
            "tcc_offset": cfg.tcc_offset,
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
            // TCC temperature
            if let Some(tcc_raw) = read_msr(MSR_TEMPERATURE, 0) {
                let tcc_offset = (tcc_raw >> 24) & 0x7F;
                result.insert("tcc_target_c".into(), (100u64 - tcc_offset).into());
            }
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
