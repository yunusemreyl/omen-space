# Changelog

All notable changes to **OMEN Space** are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
