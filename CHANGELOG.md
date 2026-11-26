# Changelog

All notable changes to **OMEN Space** are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [2.5.3] - 2026-11-26

### Added
- **Turkish Q (`TR-Q`) Keyboard Keycap Layout:** Added `TR-Q` alongside `EN` and `DE` in the RGB Studio keycap layout dropdown (`KeyboardLayout` in `keyboardrgb/helpers.rs` & `keyboard.rs`), persisted cleanly in `~/.config/omenspace/settings.json` without affecting HID usage mapping.
- **Authoritative Primax `0xFF13` McuSDK2 Key-to-LED Matrix (`#385`):** Added exact hardware key-to-LED slot maps in `rgb/primax_map.rs` (`Ralph` 167-slot map for `0461:4F03`/`4E9B`/`4F11`/`4F1E`/`4ECB` and `Cybug` 168-slot map for `0461:4E9A`) with strict null-slot zeroing, non-blocking hidraw report draining, and corrected `evdev` navigation key coordinates in `rgb/key_layout.rs`.
- **Automated Translation Parity & CI Formatting Gates:** Added a unit test enforcing 100% key-set parity across `en`, `tr`, and `es` GUI translations (fixing missing Spanish strings and two untranslated English labels), and added `cargo fmt --all -- --check` to GitHub Actions CI.

### Fixed
- **RGB Solid Color, Custom Wave, Effect Timing & Toggle Sync (`#385`):**
  - Eliminated right-side `#00FFAA` Zone 4 bleed in Per-Key `Static` mode by applying `colors[0]` uniformly across all 104 key slots.
  - Fixed `Wave (Custom Colors)` rendering static when primary and secondary colors match (added shaded trough modulation in both daemon `effects.rs` and GUI preview), and tuned `Breathing`, `Blinking`, `Marquee`, and `Ripple` animations.
  - Synced the `Off / Key / Zone / All` toggle bar when initializing from `power = false` or selecting effects/colors from `Off`, and yielded cleanly to native keyboard `Fn` lighting presets when `Hardware Effects` is enabled on Primax `0xFF13` boards.
- **Fan Dust Cleaning & `leave_max_boost` Recovery (`#385`, `#364`):** Added `refresh_active_platform_profile()` (`WMI 0x1A` refresh) inside `leave_max_boost`, immediately wrote a safe initial duty when transitioning from `max` to manual/curve modes, and seeded `PWM_MODE_MAX -> PWM_MODE_MANUAL` target RPMs from `priv->min_rpm * 100` in `driver/hp-wmi.c`.

### Changed
- **Workspace Unification & Dead-Code Cleanup:** Consolidated D-Bus proxies, `is_newer_semver`, and locale detection in `omen-types` (fixing stale `omen-cli` proxy method names); modularized `i18n/`, `monitoring/`, `updater/`, and `fan/protection.rs`; removed unused legacy assets (`omen_laptop.png`, `victus_laptop.png`, `custom.svg`), empty `omen-gui/src/lib.rs`, unused Cargo dependencies, legacy CSS selectors, and all `#[allow(dead_code)]` items.

---

## [2.5.2] - 2026-11-26

### Fixed
- **Settings Heartbeat GUI Crash (`#395`):** Fixed `settings.rs` invoking blocking `ping_daemon_sync()` from inside a Tokio worker thread (`Cannot start a runtime from within a runtime` abort every ~20–30s after opening Settings while `omen-tray` stayed running) by adding `ping_daemon_async()`, and hardened `daemon_client::block_with_timeout()` to offload cleanly whenever an active Tokio context is present on the calling thread.
- **Board `8C77` EC Max-Fan Latch, Temperature Sensor Selection & Thermal-Protection Restore (`#395`):**
  - Switched fan monitor temperature sampling (`fan/mod.rs` / `curves.rs`) to prefer real CPU package `temp1_input` sensors (`coretemp`, `k10temp`, `zenpower`) instead of transient per-core HX junction or noisy ACPI/PCH/DIMM nodes.
  - Added automatic EC max-fan latch recovery (`should_recover_ec_max_latch` + `handle_ec_override_recovery`): when actual fan speed remains >=90% max well after cooling down (<80°C) despite <=75% commanded duty, the daemon refreshes the active `platform_profile` (`WMI 0x1A`) and pulses `pwm1_enable` to release firmware VRM/chassis guard latches without user intervention.
  - Updated `set_thermal_protection(false)` to immediately restore `pre_protection_mode` if Thermal Protection was actively forcing Max Fan.
  - Moved `8C76`/`8C77`/`8C78` in `driver/hp-wmi.c` to `omen_v1_unknown_ec_thermal_params` (avoiding invalid EC `0x59` `'0'` board-revision readback) and extended `hp_wmi_hwmon_keep_alive_handler()` to refresh `platform_profile_victus_s_set_ec()` in `PWM_MODE_MANUAL` as well as `PWM_MODE_AUTO`.

---

## [2.5.1] - 2026-11-26

### Added
- **Hardware Support & Verified Boards (`#394`, `#358`):** Added full support and `boards.json` capability entries for **HyperX OMEN Gaming Laptop 15-ga0xxx (`8EDF`)** (via `victus_s_thermal_profile_boards`, `broken_omen_hpc_guid_boards`, and `no_wmi_rgb_boards` / `disable_wmi_rgb`) and promoted both **`8EDF`** and **OMEN MAX 16-ah0xxx (`8D42`)** to `verified_boards`.
- **Sonix/Microdia USB Diagnostic Detection (`#394`):** Expanded `sysmon` `lsusb` filtering to include `0c45` / `microdia` / `sonix` keyboard backlight controllers, and improved `setup.sh` error diagnostics for OpenRC systems (`#361`).

### Changed
- **Modular Daemon & GUI Refactoring:** Split `fan/mod.rs` into `fan/{mod.rs,curves.rs,hwmon.rs}` and `rgb/mod.rs` into `rgb/{mod.rs,hardware.rs,lamparray.rs,hid_per_key.rs,effects.rs,key_layout.rs}`, unified per-app & power automation loops (`game_automation.rs`, `power.rs`, `power_automation.rs`, `main.rs`) around canonical `/etc/omen-space/app_profiles.json` with legacy fallback, and streamlined `omen-gui/src/daemon_client.rs` with localized TR/EN/ES D-Bus error messages.
- **Balanced `Auto` Fan Curve Tuning (`#395`):** Refined Balanced `default_auto_step_curve()` to ramp smoothly (`88°C -> 88%`, `93°C -> 100%`) instead of pegging fans at 100% under sub-90°C load.

### Fixed
- **Max-Fan Latch & Raw EC Write Safety (`#395`, `#364`):** Added `hp_wmi_fan_speed_max_reset()` on `PWM_MODE_MAX -> PWM_MODE_MANUAL` transitions in `driver/hp-wmi.c`, avoided latching `pwm1_enable = 0` during manual/curve 100% duty (`pwm1_enable = 1, pwm1 = 255`), transitioned `0 -> 2` (`PWM_MODE_AUTO`) cleanly when leaving `Max` / Fan Dust Cleaning, restricted raw `ec_sys` writes strictly to legacy EC-fallback boards (`8E35`, `8A43`, `8A42`), and deduplicated `No HP hwmon device found` log spam.
- **Board `8EDF` Thermal Profile & Keyboard Backlight (`#394`):** Fixed `victus_s_gpu_thermal_profile_set` uninitialized status variable and tolerated positive WMI unsupported return codes (`0x05`) to eliminate `-22` setup failures on `8EDF`; guarded `no_wmi_rgb_boards` (`-ENODEV` in `hp-omen-extra.c`), removed `HPWMI_COLOR_*` fallback from `lightbar_show`/`lightbar_store`, and downgraded `0x02`/`0x03`/`0x05` probe codes to `pr_debug` so `hp_omen_extra` no longer turns off USB keyboard backlights.
- **Primax `0xFF13` & 4-Zone / LampArray RGB Fixes (`#385`, `#393`, `#358`):** Streamed software animation frames for `marquee`, `reactive`, `ripple`, `wave`, and `blinking` when `hw_effects` is set; implemented `Marquee` and `Blinking` engines; mapped all 104 slots + evdev keycodes to `(x, y)` coordinates and accepted composite USB keyboards for `Reactive`/`Ripple`; synced `config.colors` in `PerKey`/`All` mode to eliminate stale `#00FFAA` Zone 4 tinting; fixed `8A17` 4-zone boundaries & numpad key disambiguation in GUI RGB Studio; and extended OMEN MAX 16 120-lamp overrides to `8D42`, `8D87`, `8D88`, `8E9A`, and `8E9F`.
- **MUX Sysfs Readback Fallback (`#369`):** Added `write_mux_sysfs_with_fallback` in `mux.rs` to fall back cleanly to `graphics_mode` when `gpu_mux_mode` readback does not reflect the target state.

---

## [2.5.0] - 2026-11-25

### Added
- **Bespoke Performance & Fan Icon Set:** Added custom, crisp dual-tone SVG icons for every mode and control (`eco.svg`, `balanced.svg`, `performance.svg`, `fan_auto.svg`, `fan_max.svg`, `fan_custom.svg`, `fan_quiet.svg`, `fan_preset.svg`, `fan_ec.svg`, `fan_add_preset.svg`), with workspace-first dev resolution in `asset_resolver.rs` while preserving packaged `/usr/share/omen-space/assets` paths.
- **One-Click `+ New Preset` Action:** Added a dedicated **New Preset** button in the **Fan Control** header that opens the custom fan curve editor and focuses the preset name input immediately.
- **Interactive System Monitor Metric Switcher:** Added a **Power / Temp / Load** segmented toggle on the System Monitor page that switches both CPU and GPU 60-second live Cairo charts simultaneously while recording all three histories continuously.

### Changed
- **Redesigned Performance Landing & Pinned Telemetry Dock:** Replaced the three floating boxed telemetry cards with a full-width, boxless **CPU | GPU | Device Status** telemetry dock pinned flush to the bottom of the page; refreshed the Performance header with a concise hardware identity line, equal-height sculpted mode cards, a 4-column edge-aligned fan control row, and a full-width theme-adaptive custom fan curve editor.
- **Redesigned System Monitor Page:** Replaced the legacy 8-card grid with a live top summary strip (**System Power · Power Profile · Fan Mode**), unified CPU & GPU telemetry panels with theme-adaptive Cairo area charts, split **Memory & Storage** + **Battery & Power** panels, and a compact hardware/firmware footer (`OS`, `BIOS`, `EC`, `NVIDIA`).
- **Redesigned GNOME Quick Control Overlay & Cross-Device Hotkeys:** Rebuilt `omen-overlay` around native Libadwaita surfaces, keycap shortcut hints, and a live telemetry strip, with expanded cross-device evdev hotkey support.

### Fixed
- **RGB Studio Color Hydration:** Hydrated the interactive keyboard preview and color swatch directly from the current daemon state on startup with zero extra overhead.
- **Bogus Chassis Sensor Filtering & Settings Focus:** Filtered invalid `< 15°C` dummy ACPI chassis readings from the System Monitor summary strip, fixed fan subtitle truncation on wide cards, and preserved Settings selection focus across runtime language changes.

---

## [2.4.3] - 2026-11-24

### Added
- **Unified Settings Hub:** Consolidated **Update Center** and **Debug Center** as segmented sub-tabs (**General | Updates | Diagnostics**) inside **Settings**, reducing the sidebar to 6 core hardware tabs plus 1 pinned Settings item with lazy sub-tab initialization.
- **Repository Governance & Quality Tooling:** Added `SECURITY.md`, `CHANGELOG.md`, `.github/dependabot.yml`, structured GitHub Issue/PR templates, least-privilege CI `permissions`, strict `-D warnings` Clippy enforcement in CI, and modernized developer documentation under `docs/`.

### Changed
- **Streamlined Graphics Switcher (MUX & PRIME):** Redesigned `mux.rs` around a compact status pill in the header, compact mode cards aligned with the Performance page, and a single unified **Hardware & Display** group combining GPU/driver info, connected display routing, passive D3cold status, and copyable PRIME Offload launch options.
- **Codebase & Asset Cleanup:** Removed unused crate dependencies (`clap` in `omen-overlay`, `lazy_static` in `omen-space-daemon`, `glib` in `omen-tray`), dead code, unused legacy assets, and resolved all remaining workspace Clippy warnings cleanly.

---

## [2.4.2] - 2026-11-24

### Added
- **HP WMI Battery Charge Limiter (`0x2B` GBCC/SBCC):** Imported kernel `hp-wmi` `battery_charge_mode` support and daemon hysteresis limiter (`stop` / `start` thresholds, shutdown `hold` mode, and one-time full-charge bypass), supporting both firmware WMI and native `charge_control_end_threshold`.
- **Hardware-Gated GPU MUX & Advanced Optimus (`gpu_mux_supported_modes`):** Exposed real BIOS MUX mode capabilities from `hp-wmi`, gated unsupported Advanced Optimus options across GUI, Tray, and daemon (`SetGpuMode`), and exposed connected displays and passive NVIDIA dGPU `D3cold` runtime status.
- **Primax `0xFF13` Per-Key RGB Backend:** Added McuSDK2 HID lighting support for Primax per-key keyboards (`0461:4F03`, `4E9A`, `4E9B`, `4F11`, `4F1E`, `4ECB`, #385).

### Changed
- **Unified Power & Battery Page:** Moved Battery Care controls from Settings to the top of the renamed **Power & Battery** page above voltage offsets, RAPL `PL1`/`PL2`, and TCC controls.

### Fixed
- **Shift+F2 / Quick Overlay Deadlock on Wayland/X11:** Converted `omen_types::is_unix_socket_alive` to non-blocking `SOCK_NONBLOCK | SOCK_CLOEXEC` probing to avoid KDE Wayland `/tmp/.X11-unix/X1` connection stalls, preferred `:0` over `:1`, and added 250 ms hotkey debounce.

---

## [2.4.1] - 2026-11-24

### Changed
- **UI Consistency & Full TR/EN/ES Localization:** Unified CSS surface/badge tokens and tabular numerals (`tnum`), standardized `adw::Clamp` layouts across all pages, completed TR/EN/ES translations for Debug Center and RGB Studio, and aligned Quick HUD styling with the dark slate theme.

---

## [2.4.0] - 2026-11-24

### Added
- **Configurable Quick HUD Hotkey:** Rebind the global Quick HUD shortcut directly in **Settings → Quick HUD Hotkey** using physical evdev keycodes with live daemon reload (no restart required).
- **Independent CPU & GPU Custom Fan Curves:** Separate CPU and GPU curves persisted and applied independently via `fan1_target` / `fan2_target`.
- **RGB Studio & LampArray Upgrades:** Support for Darfon `0d62:30bf` (`8E9A`), physical lamp calibration, EN/DE layout toggle, and low-CPU sysfs `cycle` animations (#376, #378, #379).
- **Native OpenRC & Verified Boards:** Added native OpenRC service support (#364), Secure Boot `.ko.xz` signing improvements (#380), Debug Center clipboard full dump fix, and verified boards **8E9A**, **8E9F**, **8D2D**, **8912**, **8DCF**, **8D42**, **88F4**.

### Changed
- **~33% Faster GUI Startup:** Secondary pages built lazily on first selection (#384, #386).
- **Thermal Protection Debounce:** Requires 3 consecutive 1-second samples above critical threshold to avoid turbo-spike false triggers (#371).
