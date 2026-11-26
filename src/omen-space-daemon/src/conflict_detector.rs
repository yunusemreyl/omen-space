use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::Path;

const OWN_DKMS_PACKAGE: &str = "hp-omen-extra";

/// Other DKMS packages that also build hp-wmi.ko. DKMS reinstalls every registered
/// package on kernel upgrades and the last one wins, silently replacing our hp-wmi.
pub fn conflicting_hpwmi_dkms_packages(dkms_root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(pkgs) = std::fs::read_dir(dkms_root) else {
        return found;
    };
    for pkg in pkgs.flatten() {
        let pkg_name = pkg.file_name().to_string_lossy().to_string();
        if pkg_name == OWN_DKMS_PACKAGE {
            continue;
        }
        let Ok(versions) = std::fs::read_dir(pkg.path()) else {
            continue;
        };
        for ver in versions.flatten() {
            let Ok(conf) = std::fs::read_to_string(ver.path().join("source/dkms.conf")) else {
                continue;
            };
            let builds_hpwmi = conf.lines().any(|line| {
                let line = line.trim();
                line.starts_with("BUILT_MODULE_NAME[")
                    && line
                        .split_once('=')
                        .map(|(_, v)| matches!(v.trim().trim_matches('"'), "hp-wmi" | "hp_wmi"))
                        .unwrap_or(false)
            });
            if builds_hpwmi {
                found.push(format!(
                    "{}/{}",
                    pkg_name,
                    ver.file_name().to_string_lossy()
                ));
            }
        }
    }
    found.sort();
    found
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConflictReport {
    pub has_conflicts: bool,
    pub conflicting_processes: Vec<String>,
    pub warning_message: String,
}

pub struct ConflictDetector;

impl ConflictDetector {
    pub fn check_conflicts() -> ConflictReport {
        let known_conflicts = vec![
            (
                "nbfc",
                "Notebook FanControl (nbfc) daemon running — may conflict with HP WMI fan writes",
            ),
            (
                "throttled",
                "Lenovo/Generic throttled service running — may race with CPU thermal limits",
            ),
            (
                "ryzenadj",
                "Background ryzenadj loop active — may collide with SMU power limit writes",
            ),
            ("hp-health", "Legacy HP Health service active"),
            (
                "oghaagent",
                "HP OMEN Gaming Hub background service (Wine/Proton) active",
            ),
        ];

        let running_procs = crate::sysmon::get_running_process_names();
        let mut conflicts_found = Vec::new();

        for (proc_name, desc) in known_conflicts {
            if running_procs.contains(&proc_name.to_lowercase()) {
                conflicts_found.push(format!("{} ({})", proc_name, desc));
            }
        }

        for pkg in conflicting_hpwmi_dkms_packages(Path::new("/var/lib/dkms")) {
            let msg = format!(
                "DKMS package {} (also installs hp-wmi.ko; a kernel upgrade can replace OMEN Space's driver and break fan control. Remove with: sudo dkms remove {} --all)",
                pkg, pkg
            );
            warn!("{}", msg);
            conflicts_found.push(msg);
        }

        let has_conflicts = !conflicts_found.is_empty();
        let warning_message = if has_conflicts {
            format!(
                "Warning: {} potential conflict(s) with thermal/power/driver software detected.",
                conflicts_found.len()
            )
        } else {
            "No thermal or EC control software conflicts detected. Coexistence clear.".to_string()
        };

        if has_conflicts {
            info!("{}", warning_message);
        }

        ConflictReport {
            has_conflicts,
            conflicting_processes: conflicts_found,
            warning_message,
        }
    }
}
