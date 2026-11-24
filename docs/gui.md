# OMEN Space GUI (`omen-gui`)

Built with **Rust**, **GTK4**, and **Libadwaita**, `omen-gui` provides a responsive desktop control center without root privileges.

## Navigation Layout (`main.rs`)

- **Core Hardware Tabs (Sidebar):**
  1. **Performance** (`performance_control.rs` + compact telemetry summary): Eco / Balanced / Performance profiles, Fan Auto / Max / Custom / EC / Cleaning, independent CPU & GPU fan curves.
  2. **Power & Battery** (`undervolt.rs`): **Battery Charge Limiter** at the top (hysteresis stop/start thresholds, shutdown hold protection, one-shot 100% bypass), followed by CPU voltage offsets, RAPL `PL1`/`PL2`, and TCC ceiling.
  3. **Graphics Switcher** (`mux.rs`): Hardware-gated mode cards (Advanced Optimus & Integrated appear only if supported by BIOS), connected display routing, passive D3cold status, and one-click PRIME Offload launch copy.
  4. **System Monitor** (`monitoring.rs`): Live telemetry charts, storage blocks (`nvme*` / `sd*`), and battery health.
  5. **RGB Studio** (`keyboardrgb/` + `desktop_rgb_gui.rs`): Interactive keyboard, lightbar, and desktop zone painter + animation presets.
  6. **Profile Manager** (`appprofiles.rs`): Automatic per-app performance & fan rules (Wayland & X11).
- **Pinned Bottom Item — Unified Settings Hub (`settings.rs`):**
  - Segmented pill tabs (**General | Updates | Diagnostics**) housing application preferences (`General`), OTA + `fwupd` firmware updates (`updater.rs`), and one-click GitHub diagnostic dumps (`debug_center.rs`).

## Interface Scaling & Localization

- **UI Scale (`ui_scale.rs`):** `Ctrl+Shift+Plus` / `Minus` / `0` rescales CSS `px` lengths and `gtk-xft-dpi` cleanly between `80%` and `200%`.
- **Localization (`i18n.rs`):** Runtime-switchable **English**, **Türkçe**, and **Español**.
