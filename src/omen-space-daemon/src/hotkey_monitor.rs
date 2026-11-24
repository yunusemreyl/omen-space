use evdev::{Device, Key};
use log::{info, warn};
use omen_types::hotkey::HotkeySpec;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::unix::AsyncFd;
use zbus::Connection;

/// Modifier keys currently held on one keyboard device.
#[derive(Default, Clone, Copy)]
pub(crate) struct ModState {
    left_ctrl: bool,
    right_ctrl: bool,
    left_alt: bool,
    right_alt: bool,
    left_shift: bool,
    right_shift: bool,
    left_meta: bool,
    right_meta: bool,
}

impl ModState {
    /// Update the state for a modifier key. Returns `true` when `code` is a
    /// modifier (so the caller does not treat it as a hotkey).
    pub(crate) fn update(&mut self, code: u16, pressed: bool) -> bool {
        match code {
            29 => self.left_ctrl = pressed,
            97 => self.right_ctrl = pressed,
            56 => self.left_alt = pressed,
            100 => self.right_alt = pressed,
            42 => self.left_shift = pressed,
            54 => self.right_shift = pressed,
            125 => self.left_meta = pressed,
            126 => self.right_meta = pressed,
            _ => return false,
        }
        true
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Exact modifier match: `Shift+F2` must not fire on `Ctrl+Shift+F2`.
    fn matches(&self, spec: &HotkeySpec) -> bool {
        (self.left_ctrl || self.right_ctrl) == spec.modifiers.ctrl
            && (self.left_alt || self.right_alt) == spec.modifiers.alt
            && (self.left_shift || self.right_shift) == spec.modifiers.shift
            && (self.left_meta || self.right_meta) == spec.modifiers.meta
    }
}

/// Map a pressed key to the name broadcast in the `MacroKeyPressed` D-Bus signal.
///
/// The configured overlay binding is checked first, then the fixed OMEN/macro
/// keys. `mods` is the modifier state at the time of the press.
pub(crate) fn resolve_hotkey(
    key_code: u16,
    mods: &ModState,
    binding: &HotkeySpec,
) -> Option<&'static str> {
    if key_code == binding.key_code && mods.matches(binding) {
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

/// `~/.config/omenspace/settings.json` of the active graphical user.
fn settings_path() -> Option<PathBuf> {
    crate::notifier::DesktopNotifier::active_user_home()
        .map(|home| PathBuf::from(home).join(".config/omenspace/settings.json"))
}

fn read_hotkey(path: &Path) -> Option<HotkeySpec> {
    let content = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    Some(HotkeySpec::from_settings(&json))
}

fn apply_binding(binding: &Arc<RwLock<HotkeySpec>>, spec: HotkeySpec) {
    if let Ok(mut current) = binding.write() {
        if *current != spec {
            info!("HotkeyMonitor: overlay hotkey is now {}", spec.display());
            *current = spec;
        }
    }
}

/// Watch the user's settings file and publish the overlay binding. Polling a
/// single `stat` per second is cheaper (and more portable) than inotify here.
async fn watch_settings(binding: Arc<RwLock<HotkeySpec>>) {
    let mut path: Option<PathBuf> = None;
    let mut mtime: Option<SystemTime> = None;
    let mut last_path_check = Instant::now() - Duration::from_secs(3600);

    loop {
        let path_age = last_path_check.elapsed();
        if path.is_none() {
            // The daemon may start before anyone logs in; retry periodically.
            if path_age >= Duration::from_secs(10) {
                path = settings_path();
                last_path_check = Instant::now();
                mtime = None;
            }
        } else if path_age >= Duration::from_secs(60) {
            // Pick up a different active user after logout/login.
            let new_path = settings_path();
            last_path_check = Instant::now();
            if new_path != path {
                path = new_path;
                mtime = None;
            }
        }

        if let Some(p) = &path {
            let current = std::fs::metadata(p).and_then(|m| m.modified()).ok();
            if current != mtime {
                match current {
                    Some(_) => {
                        // Retry when the file is caught mid-write instead of
                        // resetting the hotkey to the default.
                        if let Some(spec) = read_hotkey(p) {
                            mtime = current;
                            apply_binding(&binding, spec);
                        }
                    }
                    None => {
                        mtime = None;
                        apply_binding(&binding, HotkeySpec::default());
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub struct HotkeyMonitor;

impl HotkeyMonitor {
    pub fn start(connection: Connection) {
        let binding = Arc::new(RwLock::new(HotkeySpec::default()));

        let watcher_binding = binding.clone();
        tokio::spawn(async move {
            watch_settings(watcher_binding).await;
        });

        tokio::spawn(async move {
            Self::monitor_loop(connection, binding).await;
        });
    }

    async fn monitor_loop(connection: Connection, binding: Arc<RwLock<HotkeySpec>>) {
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
                let binding = binding.clone();
                join_set.spawn(async move {
                    let Ok(file) = std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_NONBLOCK)
                        .open(&path) else { return; };
                    let Ok(async_fd) = AsyncFd::new(file) else { return; };
                    info!("HotkeyMonitor: listening to raw events on {:?}", path);
                    let mut mods = ModState::default();
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
                                        if mods.update(key_code, ev.value != 0) {
                                            continue;
                                        }

                                        if ev.value == 1 {
                                            // Key pressed
                                            let spec = binding
                                                .read()
                                                .map(|b| b.clone())
                                                .unwrap_or_default();
                                            if let Some(name) = resolve_hotkey(key_code, &mods, &spec) {
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
                                        // SYN_DROPPED: reset modifier state
                                        mods.reset();
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
