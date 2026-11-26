use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;

const CONFIG_PATH: &str = "/etc/omen-space/ryzen.json";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct RyzenConfig {
    stapm_limit: u32,
    fast_limit: u32,
    slow_limit: u32,
    tctl_temp: u32,
    all_core_co: i32,
}

impl RyzenConfig {
    fn load() -> Self {
        if let Ok(data) = fs::read_to_string(CONFIG_PATH) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }
    fn save(&self) {
        if let Some(dir) = Path::new(CONFIG_PATH).parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(CONFIG_PATH, json);
        }
    }
}

// ── AMD CPU Detection ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum RyzenFamily {
    Unknown,
    Zen1Plus,
    Raven,
    Picasso,
    Dali,
    RenoirLucienne,
    CezanneBarcelo,
    VanGogh,
    Rembrandt,
    Phoenix,
    Mendocino,
    HawkPoint,
    StrixPoint,
    StrixHalo,
    Matisse,
    Vermeer,
    RaphaelDragonRange,
    FireRange,
}

fn detect_ryzen_family() -> RyzenFamily {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mut cpu_family = 0;
    let mut model = 0;
    let mut is_amd = false;

    for line in cpuinfo.lines() {
        if line.starts_with("vendor_id") && line.contains("AuthenticAMD") {
            is_amd = true;
        }
        if line.starts_with("cpu family") {
            if let Some(idx) = line.find(':') {
                cpu_family = line[idx + 1..].trim().parse().unwrap_or(0);
            }
        }
        // exact key match: "model name" must not overwrite "model"
        if let Some((key, value)) = line.split_once(':') {
            if key.trim() == "model" {
                model = value.trim().parse().unwrap_or(0);
            }
        }
    }

    if !is_amd {
        return RyzenFamily::Unknown;
    }

    match cpu_family {
        23 => match model {
            17 => RyzenFamily::Raven,
            24 => RyzenFamily::Picasso,
            32 => RyzenFamily::Dali,
            96 | 104 => RyzenFamily::RenoirLucienne,
            113 => RyzenFamily::Matisse,
            _ => RyzenFamily::Unknown,
        },
        25 => match model {
            33 => RyzenFamily::Vermeer,
            80 => RyzenFamily::CezanneBarcelo,
            63 | 68 => RyzenFamily::Rembrandt,
            97 => RyzenFamily::RaphaelDragonRange,
            116 | 120 => RyzenFamily::Phoenix,
            117 => RyzenFamily::HawkPoint,
            144 => RyzenFamily::VanGogh,
            160 => RyzenFamily::Mendocino,
            _ => RyzenFamily::Unknown,
        },
        26 => match model {
            36 => RyzenFamily::StrixPoint,
            112 => RyzenFamily::StrixHalo,
            68 => RyzenFamily::FireRange,
            _ => RyzenFamily::Unknown,
        },
        17 => match model {
            1 | 8 => RyzenFamily::Zen1Plus,
            _ => RyzenFamily::Unknown,
        },
        _ => RyzenFamily::Unknown,
    }
}

// ── SMN Access Layer ─────────────────────────────────────────────────────────

const PCI_CONFIG_PATH: &str = "/sys/bus/pci/devices/0000:00:00.0/config";
const SMN_INDEX: u64 = 0x60;
const SMN_DATA: u64 = 0x64;

fn smn_write(addr: u32, data: u32) -> bool {
    if let Ok(mut f) = OpenOptions::new().write(true).open(PCI_CONFIG_PATH) {
        let _ = f.seek(SeekFrom::Start(SMN_INDEX));
        if f.write_all(&addr.to_le_bytes()).is_ok() {
            let _ = f.seek(SeekFrom::Start(SMN_DATA));
            if f.write_all(&data.to_le_bytes()).is_ok() {
                return true;
            }
        }
    }
    false
}

fn smn_read(addr: u32) -> Option<u32> {
    if let Ok(mut f) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(PCI_CONFIG_PATH)
    {
        let _ = f.seek(SeekFrom::Start(SMN_INDEX));
        if f.write_all(&addr.to_le_bytes()).is_ok() {
            let _ = f.seek(SeekFrom::Start(SMN_DATA));
            let mut buf = [0u8; 4];
            if f.read_exact(&mut buf).is_ok() {
                return Some(u32::from_le_bytes(buf));
            }
        }
    }
    None
}

// ── Ryzen SMU Mailbox ────────────────────────────────────────────────────────

struct SmuAddresses {
    mp1_msg: u32,
    mp1_rsp: u32,
    mp1_arg: u32,
    psmu_msg: u32,
    psmu_rsp: u32,
    psmu_arg: u32,
}

impl SmuAddresses {
    fn configure(family: RyzenFamily) -> Self {
        match family {
            RyzenFamily::Zen1Plus => Self {
                mp1_msg: 0x3B10528,
                mp1_rsp: 0x3B10564,
                mp1_arg: 0x3B10598,
                psmu_msg: 0x3B1051C,
                psmu_rsp: 0x3B10568,
                psmu_arg: 0x3B10590,
            },
            RyzenFamily::Raven
            | RyzenFamily::Picasso
            | RyzenFamily::Dali
            | RyzenFamily::RenoirLucienne
            | RyzenFamily::CezanneBarcelo => Self {
                mp1_msg: 0x3B10528,
                mp1_rsp: 0x3B10564,
                mp1_arg: 0x3B10998,
                psmu_msg: 0x3B10A20,
                psmu_rsp: 0x3B10A80,
                psmu_arg: 0x3B10A88,
            },
            RyzenFamily::VanGogh
            | RyzenFamily::Rembrandt
            | RyzenFamily::Phoenix
            | RyzenFamily::Mendocino
            | RyzenFamily::HawkPoint
            | RyzenFamily::StrixHalo => Self {
                mp1_msg: 0x3B10528,
                mp1_rsp: 0x3B10578,
                mp1_arg: 0x3B10998,
                psmu_msg: 0x3B10A20,
                psmu_rsp: 0x3B10A80,
                psmu_arg: 0x3B10A88,
            },
            RyzenFamily::StrixPoint => Self {
                mp1_msg: 0x3B10928,
                mp1_rsp: 0x3B10978,
                mp1_arg: 0x3B10998,
                psmu_msg: 0x3B10A20,
                psmu_rsp: 0x3B10A80,
                psmu_arg: 0x3B10A88,
            },
            RyzenFamily::Matisse | RyzenFamily::Vermeer => Self {
                mp1_msg: 0x3B10530,
                mp1_rsp: 0x3B1057C,
                mp1_arg: 0x3B109C4,
                psmu_msg: 0x3B10524,
                psmu_rsp: 0x3B10570,
                psmu_arg: 0x3B10A40,
            },
            RyzenFamily::RaphaelDragonRange | RyzenFamily::FireRange => Self {
                mp1_msg: 0x3B10530,
                mp1_rsp: 0x3B1057C,
                mp1_arg: 0x3B109C4,
                psmu_msg: 0x03B10524,
                psmu_rsp: 0x03B10570,
                psmu_arg: 0x03B10A40,
            },
            _ => Self {
                mp1_msg: 0,
                mp1_rsp: 0,
                mp1_arg: 0,
                psmu_msg: 0,
                psmu_rsp: 0,
                psmu_arg: 0,
            },
        }
    }
}

#[derive(PartialEq)]
enum SmuStatus {
    Ok = 0x1,
    Failed = 0xFF,
    UnknownCmd = 0xFE,
}

struct RyzenSmu {
    addrs: SmuAddresses,
}

impl RyzenSmu {
    fn new(family: RyzenFamily) -> Self {
        Self {
            addrs: SmuAddresses::configure(family),
        }
    }

    fn wait_for_response(&self, addr_rsp: u32) -> Option<u32> {
        let mut attempts = 0;
        while attempts < 1000 {
            if let Some(val) = smn_read(addr_rsp) {
                if val != 0 {
                    return Some(val);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            attempts += 1;
        }
        None
    }

    fn send_msg(
        &self,
        addr_msg: u32,
        addr_rsp: u32,
        addr_arg: u32,
        msg: u32,
        args: &[u32],
    ) -> SmuStatus {
        if addr_msg == 0 || addr_rsp == 0 || addr_arg == 0 {
            return SmuStatus::Failed;
        }

        // Wait for idle
        if self.wait_for_response(addr_rsp).is_none() {
            return SmuStatus::Failed;
        }

        // Clear response
        smn_write(addr_rsp, 0);

        // Write args (max 6)
        for (i, &arg) in args.iter().enumerate().take(6) {
            smn_write(addr_arg + (i as u32 * 4), arg);
        }

        // Send msg
        smn_write(addr_msg, msg);

        // Wait for response
        if let Some(resp) = self.wait_for_response(addr_rsp) {
            if resp == 0x1 {
                return SmuStatus::Ok;
            } else if resp == 0xFE {
                return SmuStatus::UnknownCmd;
            } else {
                return SmuStatus::Failed;
            }
        }
        SmuStatus::Failed
    }

    fn send_mp1(&self, msg: u32, args: &[u32]) -> SmuStatus {
        self.send_msg(
            self.addrs.mp1_msg,
            self.addrs.mp1_rsp,
            self.addrs.mp1_arg,
            msg,
            args,
        )
    }

    fn send_psmu(&self, msg: u32, args: &[u32]) -> SmuStatus {
        self.send_msg(
            self.addrs.psmu_msg,
            self.addrs.psmu_rsp,
            self.addrs.psmu_arg,
            msg,
            args,
        )
    }
}

// ── Limits & Curve Optimizer Application ─────────────────────────────────────

fn clamp_limit(val_mw: u32, family: RyzenFamily) -> u32 {
    let max_mw = match family {
        RyzenFamily::VanGogh | RyzenFamily::Mendocino => 30_000,
        RyzenFamily::StrixHalo => 150_000,
        RyzenFamily::RenoirLucienne
        | RyzenFamily::CezanneBarcelo
        | RyzenFamily::Rembrandt
        | RyzenFamily::Phoenix
        | RyzenFamily::HawkPoint
        | RyzenFamily::StrixPoint => 100_000,
        _ => 54_000,
    };
    val_mw.clamp(15_000, max_mw)
}

fn send_with_psmu_fallback(smu: &RyzenSmu, mp1_msg: u32, psmu_msg: u32, val: u32) -> SmuStatus {
    let args = [val];
    let res = smu.send_mp1(mp1_msg, &args);
    if res == SmuStatus::Ok {
        return res;
    }
    let fallback = smu.send_psmu(psmu_msg, &args);
    if fallback == SmuStatus::Ok {
        fallback
    } else {
        res
    }
}

// Apply power limit (STAPM/Fast/Slow) based on family logic
fn apply_stapm(smu: &RyzenSmu, family: RyzenFamily, val_mw: u32) {
    let clamped = clamp_limit(val_mw, family);
    match family {
        RyzenFamily::Raven | RyzenFamily::Picasso | RyzenFamily::Dali => {
            smu.send_mp1(0x1A, &[clamped]);
        }
        RyzenFamily::RenoirLucienne
        | RyzenFamily::VanGogh
        | RyzenFamily::CezanneBarcelo
        | RyzenFamily::Rembrandt
        | RyzenFamily::Phoenix
        | RyzenFamily::Mendocino
        | RyzenFamily::HawkPoint
        | RyzenFamily::StrixPoint => {
            send_with_psmu_fallback(smu, 0x14, 0x31, clamped);
        }
        _ => {}
    }
}

fn apply_fast(smu: &RyzenSmu, family: RyzenFamily, val_mw: u32) {
    let clamped = clamp_limit(val_mw, family);
    smu.send_mp1(0x15, &[clamped]);
}

fn apply_slow(smu: &RyzenSmu, family: RyzenFamily, val_mw: u32) {
    let clamped = clamp_limit(val_mw, family);
    smu.send_mp1(0x16, &[clamped]);
}

fn apply_tctl(smu: &RyzenSmu, family: RyzenFamily, val_c: u32) {
    if family == RyzenFamily::StrixPoint {
        let clamped = val_c.clamp(75, 105);
        smu.send_mp1(0x19, &[clamped]);
    }
}

fn apply_curve_optimizer(smu: &RyzenSmu, family: RyzenFamily, val: i32) {
    let val = val.clamp(-30, 30);
    let uval = if val < 0 {
        0x100000 - (-val as u32)
    } else {
        val as u32
    };

    let args = [uval];
    match family {
        RyzenFamily::RenoirLucienne | RyzenFamily::CezanneBarcelo => {
            if smu.send_mp1(0x55, &args) == SmuStatus::Ok {
                smu.send_psmu(0xB1, &args);
            }
        }
        RyzenFamily::Matisse | RyzenFamily::Vermeer => {
            if smu.send_mp1(0x36, &args) == SmuStatus::Ok {
                smu.send_psmu(0xB, &args);
            }
        }
        RyzenFamily::VanGogh
        | RyzenFamily::Rembrandt
        | RyzenFamily::Phoenix
        | RyzenFamily::Mendocino
        | RyzenFamily::HawkPoint => {
            if smu.send_psmu(0x5D, &args) != SmuStatus::Ok {
                smu.send_mp1(0x5D, &args);
            }
        }
        RyzenFamily::StrixPoint => {
            smu.send_mp1(0x4C, &args);
        }
        RyzenFamily::StrixHalo => {
            if smu.send_mp1(0x4C, &args) == SmuStatus::Ok {
                smu.send_psmu(0x5D, &args);
            }
        }
        RyzenFamily::RaphaelDragonRange | RyzenFamily::FireRange => {
            smu.send_psmu(0x7, &args);
        }
        _ => {
            smu.send_psmu(0x5D, &args);
        }
    }
}

// ── Service Integration ──────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RyzenService {
    config: Arc<Mutex<RyzenConfig>>,
    family: RyzenFamily,
    smu: Arc<RyzenSmu>,
}

impl RyzenService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let family = detect_ryzen_family();
        let config = RyzenConfig::load();

        let available = family != RyzenFamily::Unknown;
        if available {
            info!(
                "AMD Ryzen CPU detected (Family: {:?}). SMN Mailbox ready.",
                family
            );
        } else {
            info!("No supported AMD Ryzen CPU detected.");
        }

        let smu = Arc::new(RyzenSmu::new(family));

        let svc = Self {
            config: Arc::new(Mutex::new(config)),
            family,
            smu,
        };

        if available {
            svc.apply_all_saved().await;
        }

        Ok(svc)
    }

    async fn apply_all_saved(&self) {
        let cfg = self.config.lock().await.clone();
        if cfg.stapm_limit > 0 {
            apply_stapm(&self.smu, self.family, cfg.stapm_limit);
        }
        if cfg.fast_limit > 0 {
            apply_fast(&self.smu, self.family, cfg.fast_limit);
        }
        if cfg.slow_limit > 0 {
            apply_slow(&self.smu, self.family, cfg.slow_limit);
        }
        if cfg.tctl_temp > 0 {
            apply_tctl(&self.smu, self.family, cfg.tctl_temp);
        }
        if cfg.all_core_co != 0 {
            apply_curve_optimizer(&self.smu, self.family, cfg.all_core_co);
        }
    }
}

#[interface(name = "org.hp.omen.Ryzen")]
impl RyzenService {
    async fn set_limits(&self, stapm: u32, fast: u32, slow: u32, tctl: u32) -> String {
        if self.family == RyzenFamily::Unknown {
            warn!("SetLimits: No supported AMD Ryzen CPU.");
            return "FAIL".to_string();
        }

        {
            let mut cfg = self.config.lock().await;
            if stapm > 0 {
                cfg.stapm_limit = stapm;
                apply_stapm(&self.smu, self.family, stapm);
            }
            if fast > 0 {
                cfg.fast_limit = fast;
                apply_fast(&self.smu, self.family, fast);
            }
            if slow > 0 {
                cfg.slow_limit = slow;
                apply_slow(&self.smu, self.family, slow);
            }
            if tctl > 0 {
                cfg.tctl_temp = tctl;
                apply_tctl(&self.smu, self.family, tctl);
            }
            cfg.save();
        }

        info!(
            "RyzenService: set_limits applied (stapm={}, fast={}, slow={}, tctl={})",
            stapm, fast, slow, tctl
        );
        "OK".to_string()
    }

    async fn set_curve_optimizer(&self, all_core_co: i32) -> String {
        if self.family == RyzenFamily::Unknown {
            warn!("SetCurveOptimizer: No supported AMD Ryzen CPU.");
            return "FAIL".to_string();
        }

        let co = all_core_co.clamp(-30, 30);
        {
            let mut cfg = self.config.lock().await;
            cfg.all_core_co = co;
            apply_curve_optimizer(&self.smu, self.family, co);
            cfg.save();
        }

        info!("RyzenService: Curve Optimizer applied (all-core: {})", co);
        "OK".to_string()
    }

    async fn get_state(&self) -> String {
        let cfg = self.config.lock().await.clone();
        let available = self.family != RyzenFamily::Unknown;

        // Quick SMN read check to see if PCI access works
        let root_access = smn_read(0).is_some();

        serde_json::json!({
            "available": available,
            "root_access": root_access,
            "family_str": format!("{:?}", self.family),
            "stapm_limit": cfg.stapm_limit,
            "fast_limit":  cfg.fast_limit,
            "slow_limit":  cfg.slow_limit,
            "tctl_temp":   cfg.tctl_temp,
            "all_core_co": cfg.all_core_co,
        })
        .to_string()
    }
}
