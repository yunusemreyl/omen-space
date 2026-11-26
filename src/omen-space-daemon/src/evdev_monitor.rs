use evdev::{Device, Key};
use log::{info, warn};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::unix::AsyncFd;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct KeyEventInfo {
    pub x: f64,
    pub y: f64,
    pub timestamp: std::time::Instant,
}

pub struct EvdevMonitor {
    pub recent_keys: Arc<Mutex<Vec<KeyEventInfo>>>,
    pub active: Arc<AtomicBool>,
    pub notify: Arc<tokio::sync::Notify>,
}

/// Returns `true` when an `evdev::Device` exposes a full alpha/enter keyboard report set.
/// Accepts composite USB gaming keyboard interfaces that also advertise consumer/pointer keys (#385).
pub fn is_keyboard_device(keys: &evdev::AttributeSetRef<Key>) -> bool {
    keys.contains(Key::KEY_A)
        && keys.contains(Key::KEY_Z)
        && keys.contains(Key::KEY_SPACE)
        && keys.contains(Key::KEY_ENTER)
}

impl EvdevMonitor {
    pub fn new() -> Self {
        let recent_keys = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(tokio::sync::Notify::new());

        let keys_clone = recent_keys.clone();
        let active_clone = active.clone();
        let notify_clone = notify.clone();
        tokio::spawn(async move {
            Self::monitor_loop(keys_clone, active_clone, notify_clone).await;
        });

        Self {
            recent_keys,
            active,
            notify,
        }
    }

    pub fn set_active(&self, enabled: bool) {
        let prev = self.active.swap(enabled, Ordering::SeqCst);
        if enabled && !prev {
            self.notify.notify_one();
        } else if prev && !enabled {
            let keys = self.recent_keys.clone();
            tokio::spawn(async move {
                let mut lock = keys.lock().await;
                lock.clear();
            });
        }
    }

    async fn monitor_loop(
        keys: Arc<Mutex<Vec<KeyEventInfo>>>,
        active: Arc<AtomicBool>,
        notify: Arc<tokio::sync::Notify>,
    ) {
        loop {
            // Fix #345: When reactive lighting is not active, stay parked and do NOT poll or keep /dev/input open!
            while !active.load(Ordering::SeqCst) {
                notify.notified().await;
            }

            let mut device_paths = Vec::new();

            if let Ok(mut dir) = tokio::fs::read_dir("/dev/input").await {
                while let Ok(Some(entry)) = dir.next_entry().await {
                    let path = entry.path();
                    if path.to_string_lossy().contains("event") {
                        if let Ok(dev) = Device::open(&path) {
                            if dev.supported_keys().is_some_and(is_keyboard_device) {
                                device_paths.push(path);
                            }
                        }
                    }
                }
            }

            if device_paths.is_empty() {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }

            let mut join_set = tokio::task::JoinSet::new();
            for path in device_paths {
                let keys_c = keys.clone();
                let active_c = active.clone();
                join_set.spawn(async move {
                    let Ok(file) = std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_NONBLOCK)
                        .open(&path)
                    else {
                        return;
                    };
                    let Ok(async_fd) = AsyncFd::new(file) else {
                        return;
                    };
                    info!("EvdevMonitor: listening to raw events on {:?}", path);
                    let event_size = std::mem::size_of::<libc::input_event>();
                    let mut read_buf = vec![0u8; event_size * 32];

                    while active_c.load(Ordering::SeqCst) {
                        tokio::select! {
                            ready = async_fd.readable() => {
                                let Ok(mut guard) = ready else { break; };
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
                                            if ev.type_ == 1 && ev.value == 1 {
                                                let key_code = ev.code;
                                                if let Some((x, y)) = crate::rgb::key_layout::evdev_code_to_coords(key_code) {
                                                    let mut lock = keys_c.lock().await;
                                                    lock.push(KeyEventInfo {
                                                        x,
                                                        y,
                                                        timestamp: std::time::Instant::now(),
                                                    });
                                                    if lock.len() > 30 {
                                                        lock.remove(0);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(Ok(_)) => {}
                                    Ok(Err(e)) => {
                                        if e.raw_os_error() == Some(libc::ENODEV) || e.raw_os_error() == Some(libc::EBADF) {
                                            warn!("EvdevMonitor: device disconnected ({}); refreshing", e);
                                            break;
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                                if !active_c.load(Ordering::SeqCst) {
                                    break;
                                }
                            }
                        }
                    }
                });
            }

            while active.load(Ordering::SeqCst) {
                tokio::select! {
                    _ = join_set.join_next() => {
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                        if !active.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                }
            }
            join_set.abort_all();
        }
    }
}
