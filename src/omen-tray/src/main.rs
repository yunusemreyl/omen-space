mod i18n;

use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::MenuItem;
use i18n::t;
use log::{error, info};
use std::process::Command;
use std::sync::OnceLock;
use zbus::{Connection, Result as ZbusResult};

static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

fn spawn_task<F>(f: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Some(handle) = RUNTIME.get() {
        handle.spawn(f);
    } else {
        error!("Tokio runtime handle is not initialized");
    }
}

fn acquire_single_instance_lock() -> Option<std::fs::File> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/tmp/user-{}", unsafe { libc::getuid() }));
    let lock_path = format!("{}/omen-tray.lock", runtime_dir);

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .ok()?;

    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if res != 0 {
        return None;
    }

    Some(file)
}

fn spawn_gui() {
    let spawned: Option<std::process::Child> = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("omen-gui")))
        .and_then(|gui_path| {
            if gui_path.exists() {
                Command::new(gui_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()
            } else {
                None
            }
        })
        .or_else(|| {
            Command::new("omen-gui")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        })
        .or_else(|| {
            Command::new("/usr/bin/omen-gui")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        });

    // Reap the child once it exits so it never lingers as a zombie.
    // (Zombies previously accumulated and broke the OMEN-key toggle, which
    // relies on pgrep -x omen-gui.)
    if let Some(mut child) = spawned {
        spawn_task(async move {
            let _ = child.wait();
        });
    }
}

fn spawn_overlay() {
    let is_running = Command::new("pgrep")
        .arg("-x")
        .arg("omen-overlay")
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);

    if is_running {
        let _ = Command::new("pkill").arg("-TERM").arg("-x").arg("omen-overlay").output();
        return;
    }

    let spawned: Option<std::process::Child> = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("omen-overlay")))
        .and_then(|overlay_path| {
            if overlay_path.exists() {
                Command::new(overlay_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()
            } else {
                None
            }
        })
        .or_else(|| {
            Command::new("omen-overlay")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        });

    if let Some(mut child) = spawned {
        spawn_task(async move {
            let _ = child.wait();
        });
    }
}

async fn toggle_overlay() {
    spawn_overlay();
}

#[derive(Debug, Clone)]
struct Tray {
    power_profile: String,
    fan_mode: String,
    gpu_mode: String,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "omenspace_tray".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        "omenspace".into()
    }

    fn icon_theme_path(&self) -> String {
        "/usr/share/omen-space/assets".into()
    }

    fn title(&self) -> String {
        "OMEN SPACE".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let p_label = match self.power_profile.as_str() {
            "performance" => t("perf"),
            "power-saver" | "eco" => t("eco"),
            _ => t("balanced"),
        };
        let f_label = match self.fan_mode.as_str() {
            "max" => t("max"),
            "ec" => t("ec"),
            "custom" => t("custom"),
            _ => t("auto"),
        };
        let g_label = match self.gpu_mode.as_str() {
            "discrete" => t("tt_gpu_discrete"),
            _ => t("tt_gpu_hybrid"),
        };
        ksni::ToolTip {
            title: "OMEN Space".into(),
            description: format!("{}: {}\n{}: {}\n{}: {}", t("tt_power"), p_label, t("tt_fan"), f_label, t("tt_gpu"), g_label),
            icon_name: "omenspace".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        spawn_gui();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let cur_power = self.power_profile.as_str();
        let cur_fan = self.fan_mode.as_str();
        let cur_gpu = self.gpu_mode.as_str();

        vec![
            StandardItem {
                label: t("tray_open").into(),
                icon_name: "omenspace".into(),
                activate: Box::new(|_| {
                    spawn_gui();
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: t("tray_overlay").into(),
                icon_name: "preferences-desktop-display".into(),
                activate: Box::new(|_| {
                    spawn_task(async {
                        toggle_overlay().await;
                    });
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: t("power_profile").into(),
                submenu: vec![
                    CheckmarkItem {
                        label: t("perf").into(),
                        checked: cur_power == "performance",
                        activate: Box::new(|tray: &mut Self| {
                            tray.power_profile = "performance".into();
                            spawn_task(async {
                                set_power_profile("performance").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("balanced").into(),
                        checked: cur_power == "balanced",
                        activate: Box::new(|tray: &mut Self| {
                            tray.power_profile = "balanced".into();
                            spawn_task(async {
                                set_power_profile("balanced").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("eco").into(),
                        checked: cur_power == "power-saver" || cur_power == "eco",
                        activate: Box::new(|tray: &mut Self| {
                            tray.power_profile = "power-saver".into();
                            spawn_task(async {
                                set_power_profile("power-saver").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: t("fan_mode").into(),
                submenu: vec![
                    CheckmarkItem {
                        label: t("auto").into(),
                        checked: cur_fan == "auto",
                        activate: Box::new(|tray: &mut Self| {
                            tray.fan_mode = "auto".into();
                            spawn_task(async {
                                set_fan_mode("auto").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("max").into(),
                        checked: cur_fan == "max",
                        activate: Box::new(|tray: &mut Self| {
                            tray.fan_mode = "max".into();
                            spawn_task(async {
                                set_fan_mode("max").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("ec").into(),
                        checked: cur_fan == "ec",
                        activate: Box::new(|tray: &mut Self| {
                            tray.fan_mode = "ec".into();
                            spawn_task(async {
                                set_fan_mode("ec").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: t("gpu_mode").into(),
                submenu: vec![
                    CheckmarkItem {
                        label: t("hybrid").into(),
                        checked: cur_gpu == "hybrid",
                        activate: Box::new(|tray: &mut Self| {
                            tray.gpu_mode = "hybrid".into();
                            spawn_task(async {
                                set_gpu_mode("hybrid").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    CheckmarkItem {
                        label: t("discrete").into(),
                        checked: cur_gpu == "discrete",
                        activate: Box::new(|tray: &mut Self| {
                            tray.gpu_mode = "discrete".into();
                            spawn_task(async {
                                set_gpu_mode("discrete").await;
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: t("exit").into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| {
                    let _ = Command::new("pkill").arg("-TERM").arg("-x").arg("omen-gui").output();
                    let _ = Command::new("pkill").arg("-TERM").arg("-x").arg("omenctl").output();
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[zbus::proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
trait Power {
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn get_power_profile(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_fan_mode(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.hp.omen.Mux",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Mux"
)]
trait Mux {
    async fn set_gpu_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_gpu_info(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.hp.omen.Platform",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Platform"
)]
trait Platform {
    async fn toggle_overlay(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    async fn macro_key_pressed(&self, key_name: &str) -> zbus::Result<()>;
}

async fn get_conn() -> ZbusResult<Connection> {
    Connection::system().await
}

async fn fetch_power_profile() -> Option<String> {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = PowerProxy::new(&conn).await {
            if let Ok(json_str) = proxy.get_power_profile().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(active) = val.get("active").and_then(|v| v.as_str()) {
                        return Some(active.to_string());
                    }
                }
            }
        }
    }
    None
}

async fn fetch_fan_mode() -> Option<String> {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = FanProxy::new(&conn).await {
            if let Ok(mode) = proxy.get_fan_mode().await {
                return Some(mode.trim().to_string());
            }
        }
    }
    None
}

async fn fetch_gpu_mode() -> Option<String> {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = MuxProxy::new(&conn).await {
            if let Ok(json_str) = proxy.get_gpu_info().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(mode) = val.get("mode").and_then(|v| v.as_str()) {
                        return Some(mode.to_string());
                    }
                }
            }
        }
    }
    None
}

async fn set_power_profile(profile: &str) {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = PowerProxy::new(&conn).await {
            match proxy.set_power_profile(profile).await {
                Ok(resp) => info!("Güç profili ayarlandı ({}) -> {}", profile, resp),
                Err(e) => error!("Güç profili değiştirilemedi: {}", e),
            }
        }
    }
}

async fn set_fan_mode(mode: &str) {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = FanProxy::new(&conn).await {
            match proxy.set_fan_mode(mode).await {
                Ok(resp) => info!("Fan modu ayarlandı ({}) -> {}", mode, resp),
                Err(e) => error!("Fan modu değiştirilemedi: {}", e),
            }
        }
    }
}

async fn set_gpu_mode(mode: &str) {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = MuxProxy::new(&conn).await {
            match proxy.set_gpu_mode(mode).await {
                Ok(resp) => {
                    info!("GPU modu ayarlandı ({}) -> {}", mode, resp);
                    if resp.contains("REBOOT") {
                        let _ = Command::new("notify-send")
                            .arg("OMEN Space")
                            .arg("GPU modunun etkin olması için sistemi yeniden başlatmanız gerekiyor.")
                            .arg("-i")
                            .arg("dialog-warning")
                            .spawn();
                    }
                },
                Err(e) => error!("GPU modu değiştirilemedi: {}", e),
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let _lock_file = match acquire_single_instance_lock() {
        Some(file) => file,
        None => {
            eprintln!("omen-tray zaten çalışıyor, ikinci örnek sonlandırılıyor.");
            return;
        }
    };

    env_logger::init();
    info!("omen-tray başlatılıyor...");

    i18n::init();

    RUNTIME
        .set(tokio::runtime::Handle::current())
        .expect("Failed to initialize runtime handle");

    let initial_power = fetch_power_profile().await.unwrap_or_else(|| "balanced".into());
    let initial_fan = fetch_fan_mode().await.unwrap_or_else(|| "auto".into());
    let initial_gpu = fetch_gpu_mode().await.unwrap_or_else(|| "hybrid".into());

    let tray = Tray {
        power_profile: initial_power,
        fan_mode: initial_fan,
        gpu_mode: initial_gpu,
    };

    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    // No longer holding overlay in background

    // Listen for OMEN key presses from the zero-overhead hotkey monitor
    tokio::spawn(async move {
        if let Ok(conn) = get_conn().await {
            use futures::StreamExt;
            if let Ok(proxy) = PlatformProxy::new(&conn).await {
                if let Ok(mut stream) = proxy.receive_macro_key_pressed().await {
                    while let Some(msg) = stream.next().await {
                        if let Ok(args) = msg.args() {
                            let key_name = args.key_name();
                            if *key_name == "omen" {
                                info!("OMEN key detected, launching GUI...");
                                spawn_gui();
                            } else if *key_name == "overlay" {
                                info!("Overlay hotkey detected, toggling overlay...");
                                spawn_overlay();
                            }
                        }
                    }
                }
            }
        }
    });

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let p = fetch_power_profile().await;
        let f = fetch_fan_mode().await;
        let g = fetch_gpu_mode().await;
        if p.is_some() || f.is_some() || g.is_some() {
            handle.update(|tray| {
                if let Some(new_p) = p {
                    tray.power_profile = new_p;
                }
                if let Some(new_f) = f {
                    tray.fan_mode = new_f;
                }
                if let Some(new_g) = g {
                    tray.gpu_mode = new_g;
                }
            });
        }
    }
}
