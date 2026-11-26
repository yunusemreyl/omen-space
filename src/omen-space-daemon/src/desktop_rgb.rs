use hidapi::{HidApi, HidDevice};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const OMEN_RGB_VID: u16 = 0x103C;
const OMEN_RGB_PIDS: &[u16] = &[0x84FD, 0x84FE, 0x8602, 0x8603];

const HID_PACKET_SIZE: usize = 58;
const HID_HEADER_1: u8 = 0x3E;
const HID_HEADER_2: u8 = 0x12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DesktopRgbMode {
    Static,
    Breathing,
    Cycle,
    Blinking,
    Wave,
    Radial,
    Direct,
    Off,
}

impl DesktopRgbMode {
    pub fn to_hid_mode(&self) -> u8 {
        match self {
            DesktopRgbMode::Static => 0x01,
            DesktopRgbMode::Direct => 0x04,
            DesktopRgbMode::Off => 0x05,
            DesktopRgbMode::Breathing => 0x06,
            DesktopRgbMode::Cycle => 0x07,
            DesktopRgbMode::Blinking => 0x08,
            DesktopRgbMode::Wave => 0x09,
            DesktopRgbMode::Radial => 0x0A,
        }
    }
}

pub struct DesktopRgbController {
    api: Mutex<Option<HidApi>>,
    current_pid: Option<u16>,
}

impl Default for DesktopRgbController {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopRgbController {
    pub fn new() -> Self {
        Self {
            api: Mutex::new(None),
            current_pid: None,
        }
    }

    pub fn initialize(&mut self) -> Result<(), String> {
        let api = HidApi::new().map_err(|e| format!("Failed to init HID API: {}", e))?;

        let mut found_pid = None;
        for device_info in api.device_list() {
            if device_info.vendor_id() == OMEN_RGB_VID
                && OMEN_RGB_PIDS.contains(&device_info.product_id())
            {
                info!(
                    "Found OMEN Desktop RGB Controller (PID: {:04X})",
                    device_info.product_id()
                );
                found_pid = Some(device_info.product_id());
                break;
            }
        }

        if let Some(pid) = found_pid {
            self.current_pid = Some(pid);
            *self.api.lock().unwrap_or_else(|e| e.into_inner()) = Some(api);
            return Ok(());
        }

        Err("No OMEN Desktop RGB device found".to_string())
    }

    pub fn is_available(&self) -> bool {
        self.current_pid.is_some()
    }

    fn open_device(&self) -> Result<HidDevice, String> {
        let guard = self.api.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(api) = guard.as_ref() {
            if let Some(pid) = self.current_pid {
                // Usually interface 0
                match api.open(OMEN_RGB_VID, pid) {
                    Ok(dev) => return Ok(dev),
                    Err(e) => return Err(format!("Failed to open HID device: {}", e)),
                }
            }
        }
        Err("API not initialized".to_string())
    }

    pub fn set_static_colors(&self, colors: &[(u8, u8, u8)], brightness: u8) -> Result<(), String> {
        let mut packet = [0u8; HID_PACKET_SIZE];

        packet[0] = 0x00; // Report ID
        packet[1] = HID_HEADER_1;
        packet[2] = HID_HEADER_2;
        packet[3] = DesktopRgbMode::Static.to_hid_mode();
        packet[4] = 0x01; // Color count
        packet[5] = 0x01; // Current color

        // Zone colors (up to 7 zones starting at byte 8)
        for (i, &(r, g, b)) in colors.iter().enumerate().take(7) {
            let offset = 8 + (i * 3);
            packet[offset] = r;
            packet[offset + 1] = g;
            packet[offset + 2] = b;
        }

        packet[48] = brightness.clamp(25, 100);
        packet[49] = 0x02; // Type = Static
        packet[54] = 0x00; // All LEDs
        packet[55] = 0x01; // Power ON
        packet[56] = 0x00; // Theme custom

        let dev = self.open_device()?;
        match dev.write(&packet) {
            Ok(bytes) if bytes >= HID_PACKET_SIZE - 1 => {
                debug!("Successfully sent HID packet ({} bytes)", bytes);
                Ok(())
            }
            Ok(bytes) => Err(format!("Wrote incomplete packet: {} bytes", bytes)),
            Err(e) => Err(format!("Failed to write to HID device: {}", e)),
        }
    }
}
