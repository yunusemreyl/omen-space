use evdev::{Device, Key};
use log::{info, warn};
use zbus::Connection;
use std::os::unix::fs::OpenOptionsExt;
use std::os::fd::AsRawFd;
use tokio::io::unix::AsyncFd;

/// Map a pressed key to the name broadcast in the `MacroKeyPressed` D-Bus signal.
///
/// `shift_held` is true while either Shift key is down.
pub(crate) fn resolve_hotkey(key_code: u16, shift_held: bool) -> Option<&'static str> {
    // Shift+F2 opens/closes the quick HUD overlay (handled by omen-tray).
    if (key_code == Key::KEY_F2.code() || key_code == 60) && shift_held {
        return Some("overlay");
    }
    // 148 = KEY_PROG1 (Omen Key mapped by hwdb / hp-omen-extra)
    // 200 = raw XF86Launch2 scancode on some boards without hwdb
    // 149 = KEY_PROG2 (P1/P2/Macro)
    // 140 = KEY_CALC (Calculator)
    // 256 = KEY_PROG3
    // Issue #342: Support 8A17 / Omen 17 programmable keys P1..P6
    match key_code {
        148 | 200 => Some("omen"),
        149 => Some("prog2"),
        140 => Some("calc"),
        256 => Some("prog3"),
        202 | 183 => Some("p1"), // KEY_PROG1 / KEY_F13
        203 | 184 => Some("p2"), // KEY_PROG2 / KEY_F14
        204 | 185 => Some("p3"), // KEY_PROG3 / KEY_F15
        205 | 186 => Some("p4"), // KEY_PROG4 / KEY_F16
        187 => Some("p5"),       // KEY_F17
        188 => Some("p6"),       // KEY_F18
        _ => None,
    }
}

pub struct HotkeyMonitor;

impl HotkeyMonitor {
    pub fn start(connection: Connection) {
        tokio::spawn(async move {
            Self::monitor_loop(connection).await;
        });
    }

    async fn monitor_loop(connection: Connection) {
        loop {
            let mut matching_devices: Vec<std::path::PathBuf> = Vec::new();
            let mut omen_device_found = false;

            if let Ok(mut dir) = tokio::fs::read_dir("/dev/input").await {
                while let Ok(Some(entry)) = dir.next_entry().await {
                    let path = entry.path();
                    if path.to_string_lossy().contains("event") {
                        if let Ok(dev) = Device::open(&path) {
                            let has_prog1 = dev.supported_keys()
                                .is_some_and(|k| k.contains(Key::KEY_PROG1));
                            let is_keyboard = dev.supported_keys().is_some_and(|k| {
                                k.contains(Key::KEY_A)
                                    || k.contains(Key::KEY_F2)
                                    || k.contains(Key::KEY_PROG1)
                                    || k.contains(Key::KEY_CALC)
                                    || k.contains(Key::KEY_F13)
                            });
                            let is_mouse = dev.supported_relative_axes().is_some_and(|a| {
                                a.contains(evdev::RelativeAxisType::REL_X)
                                    || a.contains(evdev::RelativeAxisType::REL_Y)
                            });
                            if has_prog1 {
                                omen_device_found = true;
                            }
                            if is_keyboard && !is_mouse {
                                matching_devices.push(path);
                            }
                        }
                    }
                }
            }

            if !omen_device_found {
                warn!(
                    "HotkeyMonitor: No device advertising KEY_PROG1 (Omen key) found. \
                     Check that the hp-omen-extra kernel module is loaded (`lsmod | grep hp_omen`) \
                     and that `udevadm hwdb --update && udevadm trigger` has been run. \
                     Raw scancode 200 / keycode 148 will still be caught if the HP WMI \
                     hotkeys device is present. (Issue #266)"
                );
            }

            if matching_devices.is_empty() {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                continue;
            }

            let mut join_set = tokio::task::JoinSet::new();
            for path in matching_devices {
                let conn = connection.clone();
                join_set.spawn(async move {
                    let Ok(file) = std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_NONBLOCK)
                        .open(&path) else { return; };
                    let Ok(async_fd) = AsyncFd::new(file) else { return; };
                    info!("HotkeyMonitor: listening to raw events on {:?}", path);
                    let mut left_shift = false;
                    let mut right_shift = false;
                    let event_size = std::mem::size_of::<libc::input_event>();
                    let mut read_buf = vec![0u8; event_size * 32];

                    loop {
                        let Ok(mut guard) = async_fd.readable().await else { break; };
                        let try_res = guard.try_io(|_| {
                            let fd = async_fd.get_ref().as_raw_fd();
                            let n = unsafe {
                                libc::read(fd, read_buf.as_mut_ptr() as *mut libc::c_void, read_buf.len())
                            };
                            if n < 0 {
                                Err(std::io::Error::last_os_error())
                            } else {
                                Ok(n as usize)
                            }
                        });

                        match try_res {
                            Ok(Ok(bytes_read)) if bytes_read >= event_size => {
                                let count = bytes_read / event_size;
                                let events = unsafe {
                                    std::slice::from_raw_parts(read_buf.as_ptr() as *const libc::input_event, count)
                                };
                                for ev in events {
                                    // EV_KEY = 1
                                    if ev.type_ == 1 {
                                        let key_code = ev.code;
                                        if key_code == Key::KEY_LEFTSHIFT.code() || key_code == 42 {
                                            left_shift = ev.value != 0;
                                        } else if key_code == Key::KEY_RIGHTSHIFT.code() || key_code == 54 {
                                            right_shift = ev.value != 0;
                                        }
                                        let shift_held = left_shift || right_shift;

                                        if ev.value == 1 {
                                            // Key pressed
                                            if let Some(name) = resolve_hotkey(key_code, shift_held) {
                                                info!("HotkeyMonitor: Detected Hotkey / Macro Key: {}", name);
                                                let _ = conn.emit_signal(
                                                    None::<&str>,
                                                    "/org/hp/omen/Platform",
                                                    "org.hp.omen.Platform",
                                                    "MacroKeyPressed",
                                                    &(name),
                                                ).await;
                                            }
                                        }
                                    } else if ev.type_ == 0 && ev.code == 3 {
                                        // SYN_DROPPED: reset shift state
                                        left_shift = false;
                                        right_shift = false;
                                    }
                                }
                            }
                            Ok(Ok(_)) => {}
                            Ok(Err(e)) => {
                                if e.raw_os_error() == Some(libc::ENODEV) || e.raw_os_error() == Some(libc::EBADF) {
                                    warn!("HotkeyMonitor: device disconnected ({}); refreshing", e);
                                    break;
                                }
                            }
                            Err(_would_block) => {}
                        }
                    }
                });
            }

            let _ = join_set.join_next().await;
            join_set.abort_all();
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }
}
