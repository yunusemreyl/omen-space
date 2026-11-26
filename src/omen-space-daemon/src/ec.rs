use log::{debug, info, warn};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

const EC_PATH: &str = "/sys/kernel/debug/ec/ec0/io";

const REG_FAN1_SPEED_PCT: u64 = 0x2E;
const REG_FAN2_SPEED_PCT: u64 = 0x2F;
const REG_FAN1_SPEED_SET: u64 = 0x34;
const REG_FAN2_SPEED_SET: u64 = 0x35;
const REG_BIOS_CONTROL: u64 = 0x62;
const REG_TIMER: u64 = 0x63;
const REG_FAN_BOOST: u64 = 0xEC;
const REG_FAN_STATE: u64 = 0xF4;

/// Legacy 2021–2023 boards whose firmware lacks full hwmon / WMI thermal control
/// and relies on `ec_sys` registers (`0x59`, `0x2E/0x2F`, `0x62`, `0xF4`).
/// All other boards (including `8C77`, `8EDF`, Transcend, OMEN 16/17/MAX) MUST
/// NOT receive raw EC writes because register offsets (e.g. `0x95`) collide with
/// unrelated EC data such as battery part numbers (#395).
const EC_FALLBACK_BOARDS: &[&str] = &["8E35", "8A43", "8A42"];

pub struct LinuxEcController {
    has_ec_access: bool,
    board_id: String,
    allows_direct_writes: bool,
}

impl LinuxEcController {
    pub fn new() -> Self {
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .unwrap_or_else(|_| "UNKNOWN".to_string())
            .trim()
            .to_string();

        let allows_direct_writes = Self::allows_direct_ec_writes(&board_id);
        let mut ec = LinuxEcController {
            has_ec_access: false,
            board_id,
            allows_direct_writes,
        };
        if ec.allows_direct_writes {
            ec.ensure_ec_sys();
        }
        ec.has_ec_access = Path::new(EC_PATH).exists();
        ec
    }

    pub fn needs_ec_fallback(&self) -> bool {
        Self::needs_ec_fallback_for_board(&self.board_id)
    }

    /// Static form used by sysmon for fan RPM fallback without a controller instance.
    /// Board 8A42, 8A43, and 8E35 have no hwmon fan_input paths; EC registers
    /// 0x2E/0x2F carry duty-cycle % that we use as an approximate RPM source.
    pub fn needs_ec_fallback_for_board(board_id: &str) -> bool {
        let upper = board_id.trim().to_uppercase();
        EC_FALLBACK_BOARDS.iter().any(|&b| b == upper)
    }

    /// Only allow raw `ec_sys` writes on vetted legacy EC-fallback boards (#395).
    pub fn allows_direct_ec_writes(board_id: &str) -> bool {
        Self::needs_ec_fallback_for_board(board_id)
    }

    pub fn has_ec_access(&self) -> bool {
        self.has_ec_access
    }

    fn ensure_ec_sys(&self) {
        if !self.allows_direct_writes || Path::new(EC_PATH).exists() {
            return;
        }

        let debugfs_base = "/sys/kernel/debug";
        if !Path::new(debugfs_base).exists() {
            let _ = Command::new("mount")
                .args(["-t", "debugfs", "none", debugfs_base])
                .output();
        }

        let _ = Command::new("modprobe")
            .args(["ec_sys", "write_support=1"])
            .output();
    }

    fn write_byte(&mut self, reg: u64, value: u8) -> bool {
        if !self.allows_direct_writes || !self.has_ec_access {
            return false;
        }

        match OpenOptions::new().read(true).write(true).open(EC_PATH) {
            Ok(mut file) => {
                if file.seek(SeekFrom::Start(reg)).is_ok() && file.write(&[value]).is_ok() {
                    let _ = file.flush();
                    debug!("EC write_byte success at 0x{:02X} = 0x{:02X}", reg, value);
                    return true;
                }
            }
            Err(_) => {
                warn!("EC access lost on write. Kernel lockdown?");
                self.has_ec_access = false;
            }
        }
        false
    }

    pub async fn restore_auto_mode(&mut self) -> bool {
        if !self.allows_direct_writes || !self.has_ec_access {
            return false;
        }

        self.write_byte(REG_FAN1_SPEED_SET, 0x00);
        self.write_byte(REG_FAN2_SPEED_SET, 0x00);
        self.write_byte(REG_FAN1_SPEED_PCT, 0x00);
        self.write_byte(REG_FAN2_SPEED_PCT, 0x00);
        self.write_byte(REG_FAN_BOOST, 0x00);

        if !self.write_byte(REG_FAN_STATE, 0x00) {
            return false;
        }
        if !self.write_byte(REG_BIOS_CONTROL, 0x00) {
            return false;
        }

        self.write_byte(REG_TIMER, 0x78);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.write_byte(REG_FAN_STATE, 0x00);

        true
    }

    pub fn set_perf_mode(&mut self, mode: &str) -> bool {
        if !self.allows_direct_writes || !self.has_ec_access {
            return false;
        }

        let val = match mode.to_lowercase().as_str() {
            "performance" | "max" => 0x31,
            "cool" | "eco" => 0x50,
            _ => 0x30,
        };
        info!(
            "Using EC Fallback for {} to set thermal profile: 0x{:02X} at 0x59",
            self.board_id, val
        );
        self.write_byte(0x59, val)
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxEcController;

    #[test]
    fn direct_ec_writes_only_allowed_on_legacy_fallback_boards() {
        for allowed in ["8E35", "8a43", " 8A42 "] {
            assert!(
                LinuxEcController::allows_direct_ec_writes(allowed),
                "legacy EC board '{}' should be allowed",
                allowed
            );
        }
        for blocked in [
            "8C77", "8EDF", "8D42", "8D88", "8DCF", "8A4C", "8A17", "UNKNOWN", "",
        ] {
            assert!(
                !LinuxEcController::allows_direct_ec_writes(blocked),
                "modern/non-legacy board '{}' must NEVER get raw EC writes (#395)",
                blocked
            );
        }
    }
}
