use evdev::{Device, Key};
use futures::StreamExt;
use log::{info, warn};
use zbus::Connection;

/// Map a pressed key to the name broadcast in the `MacroKeyPressed` D-Bus signal.
///
/// `shift_held` is true while either Shift key is down. It is a free function (not
/// inlined in the event loop) so the unit tests exercise the real mapping.
pub(crate) fn resolve_hotkey(key_code: u16, shift_held: bool) -> Option<&'static str> {
    // Shift+F2 opens/closes the quick HUD overlay (handled by omen-tray).
    if (key_code == Key::KEY_F2.code() || key_code == 60) && shift_held {
        return Some("overlay");
    }
    // 148 = KEY_PROG1 (Omen Key mapped by hwdb / hp-omen-extra)
    // 200 = raw XF86Launch2 scancode on some boards without hwdb
    // 149 = KEY_PROG2 (P1/P2/Macro)
    // 140 = KEY_CALC (Calculator)
    match key_code {
        148 | 200 => Some("omen"),
        149 => Some("prog2"),
        140 => Some("calc"),
        256 => Some("prog3"),
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
            let mut streams = Vec::new();
            let mut omen_device_found = false;
            if let Ok(mut dir) = tokio::fs::read_dir("/dev/input").await {
                while let Ok(Some(entry)) = dir.next_entry().await {
                    let path = entry.path();
                    if path.to_string_lossy().contains("event") {
                        if let Ok(dev) = Device::open(&path) {
                            let has_prog1 = dev.supported_keys()
                                .is_some_and(|k| k.contains(Key::KEY_PROG1));
                            let is_keyboard = dev.supported_keys().is_some_and(|k| {
                                k.contains(Key::KEY_A) || k.contains(Key::KEY_F2) || k.contains(Key::KEY_PROG1) || k.contains(Key::KEY_CALC)
                            });
                            let is_mouse = dev.supported_relative_axes().is_some_and(|a| {
                                a.contains(evdev::RelativeAxisType::REL_X) || a.contains(evdev::RelativeAxisType::REL_Y)
                            });
                            if has_prog1 {
                                omen_device_found = true;
                            }
                            if is_keyboard && !is_mouse {
                                if let Ok(stream) = dev.into_event_stream() {
                                    info!("HotkeyMonitor: listening to {:?}", path);
                                    streams.push(stream);
                                }
                            }
                        }
                    }
                }
            }

            // Issue #266: If no device advertising KEY_PROG1 was found, the hp-wmi
            // hwdb remapping may not be active (e.g. hp-omen-extra kernel module not
            // loaded, or udev hwdb not reloaded after install). Log a diagnostic
            // warning so users can trace the issue; hotkey_monitor still listens to
            // all keyboards and will catch the raw AT scancode (200) if present.
            if !omen_device_found {
                warn!(
                    "HotkeyMonitor: No device advertising KEY_PROG1 (Omen key) found. \
                     Check that the hp-omen-extra kernel module is loaded (`lsmod | grep hp_omen`) \
                     and that `udevadm hwdb --update && udevadm trigger` has been run. \
                     Raw scancode 200 / keycode 148 will still be caught if the HP WMI \
                     hotkeys device is present. (Issue #266)"
                );
            }

            if streams.is_empty() {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                continue;
            }

            let mut select_all = futures::stream::select_all(streams);
            let mut left_shift = false;
            let mut right_shift = false;

            while let Some(Ok(event)) = select_all.next().await {
                if let evdev::InputEventKind::Key(key) = event.kind() {
                    let key_code = key.code();

                    // Track Shift key state
                    if key_code == Key::KEY_LEFTSHIFT.code() {
                        left_shift = event.value() != 0;
                    } else if key_code == Key::KEY_RIGHTSHIFT.code() {
                        right_shift = event.value() != 0;
                    }
                    let shift_held = left_shift || right_shift;

                    if event.value() == 1 { // Key press
                        let key_name = resolve_hotkey(key_code, shift_held);

                        if let Some(name) = key_name {
                            info!("HotkeyMonitor: Detected Hotkey / Macro Key: {}", name);
                            let _ = connection.emit_signal(
                                None::<&str>,
                                "/org/hp/omen/Platform",
                                "org.hp.omen.Platform",
                                "MacroKeyPressed",
                                &(name),
                            ).await;
                        }
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }
}
