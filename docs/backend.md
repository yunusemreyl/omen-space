# OMEN Space Backend Daemon (`omen-space-daemon`)

`omen-space-daemon` is the privileged Rust (`tokio` + `zbus`) system service executing all hardware operations behind the `org.hp.omen.*` D-Bus interfaces.

## Key Modules (`src/omen-space-daemon/src/`)

| File | Responsibility |
| :--- | :--- |
| `main.rs` | Initializes logging, hardware database (`boards.json`), battery hysteresis loop, hotkey monitor, and D-Bus services. |
| `platform.rs` | Top-level `org.hp.omen.Platform` service (`BatteryCare`, `BatteryLimitConfig`, `BypassBatteryLimit`, capability checks, hotkey events). |
| `battery_limit.rs` | Pure hysteresis state machine supporting WMI `battery_charge_mode` (`inhibit`/`hold`/`auto`) and native `charge_control_end_threshold`. |
| `mux.rs` | `org.hp.omen.Mux` service: resolves BIOS `gpu_mux_supported_modes` / `boards.json` capabilities, enforces valid switches, computes `reboot_pending`, and reads passive DRM/PCI D3cold topology. |
| `fan/` (`mod.rs`, `curves.rs`, `hwmon.rs`, `protection.rs`) | `org.hp.omen.Fan` service: BIOS thermal profiles, independent CPU & GPU custom fan curves, EC delegation, fan dust cleaning, and 3s-debounced thermal protection / EC max-latch recovery. |
| `power.rs` / `undervolt.rs` / `ryzen.rs` | Intel RAPL (`PL1`/`PL2`), `TCC Offset`, MSR voltage offsets (respecting `UV_LOCK`), GPU TGP/PPAB, and AMD Ryzen SMU tuning. |
| `game_automation.rs` / `power_automation.rs` | Shared per-app profile switching and AC/DC power-source automation (`/etc/omen-space/app_profiles.json`). |
| `rgb/` (`mod.rs`, `hardware.rs`, `lamparray.rs`, `hid_per_key.rs`, `primax_map.rs`, `effects.rs`, `key_layout.rs`) & `desktop_rgb.rs` | 4-zone, 1-zone, lightbar, Desktop RGB, and HID per-key backends (Darfon, HP LampArray, SteelSeries, Primax `0xFF13` McuSDK2) with software animation & evdev reactive engines. |
| `hotkey_monitor.rs` / `evdev_monitor.rs` | Cross-device evdev listeners for the OMEN key, Quick HUD hotkey (`~/.config/omenspace/settings.json`), and reactive/ripple RGB typing events. |
| `sysmon/` | Non-waking telemetry collectors (CPU, NVML GPU, NVMe/SATA drives, battery health, chassis sensor). |

## Security Boundaries

- **D-Bus Policy (`data/org.hp.omen.conf`):** Restricts service ownership to `root` and method invocations to `root`, `omen-hw`, `wheel`, and `adm`.
- **Input Validation:** Numeric bounds and allowlists are enforced in Rust before any sysfs, MSR, or WMI write occurs.
