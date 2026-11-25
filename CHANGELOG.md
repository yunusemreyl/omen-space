# Changelog

All notable changes to **OMEN Space** are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
