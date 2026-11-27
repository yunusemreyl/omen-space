<div align="center">
  <img src="images/omenspace.png" alt="OMEN Space Logo" width="110" />

  # OMEN Space

  **Native Linux hardware control center for HP OMEN, Victus & Transcend laptops.**  
  Built from the ground up in **Rust** with **GTK4 & Libadwaita** for near-zero overhead.

  <p>
    <a href="https://github.com/yunusemreyl/omen-space/releases/latest">
      <img src="https://img.shields.io/badge/Release-v2.6.2-e11d48?style=flat-square" alt="Release v2.6.2" />
    </a>
    <a href="LICENSE">
      <img src="https://img.shields.io/badge/License-GPL--3.0-10b981?style=flat-square" alt="License" />
    </a>
    <img src="https://img.shields.io/badge/Written%20in-Rust-f97316?style=flat-square" alt="Rust" />
    <img src="https://img.shields.io/badge/UI-GTK4%20%2B%20Libadwaita-3b82f6?style=flat-square" alt="GTK4" />
  </p>

  <p>
    <b>English</b> · <a href="README.es.md">Español</a>
  </p>

  <br />
  <img src="images/perf.png" width="92%" alt="OMEN Space — Performance & Pinned Telemetry Dock" />
</div>

---

## Overview

**OMEN Space** replaces bloated background utilities with a ~3 MB native Rust daemon and a responsive GTK4 desktop interface. It talks directly to HP WMI/ACPI interfaces, Embedded Controller (EC) registers, Intel RAPL / MSR, AMD Ryzen SMU, and USB HID lighting controllers over a privileged D-Bus service.

| Module | Capabilities |
| :--- | :--- |
| **Performance & Cooling** | `Eco`, `Balanced`, and `Performance` BIOS thermal profiles; software-smart `Auto` cooling, `Max` fan mode, independent CPU & GPU custom multi-point fan curves with one-click **New Preset** creation, **Delegate to EC**, fan dust-cleaning mode, and a boxless **pinned bottom telemetry dock** (`CPU`, `GPU`, `RAM`, `NVMe`, `Battery`). |
| **System Monitor** | Live **Power / Temp / Load** 60-second area charts for CPU & GPU, live system power & active mode summary strip, per-drive storage utilization (`NVMe`/`SSD`/`HDD`), detailed **Battery & Power** health telemetry, and firmware metadata (`OS`, `BIOS`, `EC`, `NVIDIA`). |
| **Power & Battery** | HP WMI `0x2B` / native battery charge limiter with hysteresis (`stop`/`start` thresholds, shutdown `hold` protection, one-shot 100% bypass), real-time Intel RAPL `PL1`/`PL2` power limits, `TCC Offset`, MSR undervolting (`UV_LOCK` detection), GPU Dynamic Boost (`cTGP`/`PPAB`), and AMD Ryzen SMU tuning. |
| **Graphics Switcher (MUX & PRIME)** | Hardware-gated **Hybrid**, **Discrete (dGPU)**, **Advanced Optimus**, and **Integrated** switching backed by BIOS `/sys/devices/platform/hp-wmi/gpu_mux_supported_modes`, live display routing, passive `D3cold` state, and one-click Steam/app **PRIME Offload** launch copy. |
| **RGB Studio** | 4-Zone OMEN, Single-Zone Victus, chassis Lightbar, Desktop RGB, and HID Per-Key RGB (Darfon `0d62:54bf` / `0d62:30bf` + Primax `0xFF13` McuSDK2) with daemon-hydrated preview, named per-key colour presets, physical lamp calibration, EN/DE/TR-Q layout selector, and low-latency animations. |
| **Quick Control HUD (`Shift+F2`, configurable)** | Native Libadwaita floating overlay with keyboard shortcuts (`1–3`, `Q/W/E`), cross-device evdev hotkey support, and a live telemetry bar synchronized with the system tray and main GUI. |
| **Unified Settings Hub** | Clean **General \| Updates \| Diagnostics** segmented hub with one-click GitHub **Full System Dump**, OTA + `fwupd` (LVFS) update checks, conflict detection, and per-app profiles. |

---

## Installation

Run the one-line installer to automatically detect your distribution, install build dependencies, compile the workspace, and register the DKMS kernel drivers (`hp-wmi` & `hp-omen-extra`):

```bash
curl -sSL https://raw.githubusercontent.com/yunusemreyl/omen-space/main/install.sh | sudo bash -s -- --stable
```

To track the latest commits directly from `main`:

```bash
curl -sSL https://raw.githubusercontent.com/yunusemreyl/omen-space/main/install.sh | sudo bash -s -- --canary
```

<details>
<summary><b>Manual Build, Arch Linux (PKGBUILD), NixOS & Uninstall</b></summary>

#### Manual Install (Ubuntu/Debian, Fedora, Arch, openSUSE)
```bash
git clone https://github.com/yunusemreyl/omen-space.git
cd omen-space
sudo ./setup.sh install
```

#### Arch Linux (`makepkg`)
```bash
git clone https://github.com/yunusemreyl/omen-space.git
cd omen-space
makepkg -si
```

#### NixOS (Flakes)
```bash
nix profile install github:yunusemreyl/omen-space
```

#### Update or Uninstall
```bash
sudo ./setup.sh update      # Pull latest changes, rebuild, and reinstall
sudo ./setup.sh uninstall   # Remove daemon, UI, and DKMS modules cleanly
```
</details>

---

## Interface Gallery

<p align="center">
  <img src="images/monitor.png" width="49%" alt="System Monitor" />
  <img src="images/undervolt.png" width="49%" alt="Power & Battery" />
</p>
<p align="center">
  <img src="images/mux.png" width="49%" alt="Graphics Switcher (MUX)" />
  <img src="images/keyboard.png" width="49%" alt="RGB Studio" />
</p>
<p align="center">
  <img src="images/appprofiles.png" width="49%" alt="Profile Manager" />
  <img src="images/settings.png" width="49%" alt="Settings Hub" />
</p>

<details>
<summary><b>More Views (Quick Control HUD, Updates, Diagnostics & Interactive CLI)</b></summary>
<br />
<p align="center">
  <img src="images/updater.png" width="49%" alt="Settings — Updates" />
  <img src="images/diagnostic.png" width="49%" alt="Settings — Diagnostics" />
</p>
<p align="center">
  <img src="images/overlay.png" width="46%" alt="OMEN Quick Control HUD" />
  <img src="images/cli.png" width="49%" alt="Interactive CLI" />
</p>
</details>

---

## Architecture

| Component | Role |
| :--- | :--- |
| **`omen-space-daemon`** | Root service (`systemd` & native `OpenRC`) managing WMI, ACPI, sysfs, EC, MSR/RAPL, and HID devices over D-Bus (`org.hp.omen.*`). |
| **`omen-gui`** | Native GTK4 + Libadwaita desktop application with lazy page startup, scalable UI (`Ctrl+Shift+Plus` / `Minus`), and instant localization (**English**, **Türkçe**, **Español**). |
| **`omen-overlay`** | Lightweight Quick Control HUD (default `Shift+F2`, configurable in Settings) synchronized in real time with the daemon and main GUI. |
| **`omen-tray`** | StatusNotifierItem system tray applet for quick profile switching and background hotkey handling. |
| **`omen-cli`** | Scriptable command-line interface and interactive TUI (`omen-cli system info`, `omen-cli overlay toggle`, `omen-cli system check-update`). |
| **`hp-omen-extra`** | Custom DKMS kernel driver package (`hp-wmi` + `hp-omen-extra`) with Secure Boot MOK auto-signing (`.ko.xz`/`.ko.zst`) exposing fan PWM, thermal profiles, GPU TGP/PPAB, and RGB zones. |

---

## What's New in v2.6.2

- **Performance Page Power Sync & Opt-In Battery Eco Switching (`#398`):** Moved AC/battery switching directly onto the **Performance** page as **Sync with Battery Status** (now opt-in / off by default so unplugging never unexpectedly throttles manual setups) alongside **Sync with System Power Management** (`SetSystemPowerSync`, `omen-cli power system-sync on|off`). When `TLP` or `auto-cpufreq` is active or enabled on the host, the system-sync switch automatically hides and displays which manager controls system power.
- **Native Hardware `LampArray` Grid Effect Rendering & Column Fix (`#401`):** Software RGB effects now render natively onto the physical `LampArray` `(x, y)` row/column grid (`build_lamp_grid`, `compute_point_effect_rgb`) instead of quantizing through the fixed 104-slot `KEY_COORDS` map, eliminating the two dead columns on OMEN MAX 16 (`8D88`, 120 lamps in 6×20) and fixing the 104-slot `KEY_COORDS` `.5` rounding column holes (`[15, 17, 19]`).
- **Primax McuSDK2 (`0461:4F03`) Effect Latch, Reactive Wakeup & Paint Sync (`#364`):** Fixed `plan_primax_packets` so every changed frame writes all 9 ordered R/G/B pages ending with the `0x07` chunk 2 firmware latch (fixing frozen `Starlight`/`Marquee`/`Ripple`/`Raindrop` and dark `Reactive`/`Breathing`/`Blinking` on `B == 0` colors while still sending zero packets on idle frames), changed `evdev` keypress wakeups to `Notify::notify_one()`, and synced RGB Studio's effect dropdown to **Per-Key Custom** when painting individual keys/zones so re-selecting **Wave** always works immediately.
- **Verified Victus 15-fb2xxx (`8D07`) Support (`#399`, `#402`) & Firmware Custom Profile Fix (`#400`):** Added board `8D07` to `driver/hp-wmi.c` and `boards.json` (`verified_boards`, dual-fan WMI, single-zone RGB, best-effort GPU cTGP/PPAB handling on AMD iGPU builds), and incorporated [@Dmitriy-Romanov](https://github.com/Dmitriy-Romanov)'s PR `#400` mapping firmware `"custom"` profile readbacks to `"balanced"`.

---

## Previously in v2.6.1

- **Ultra-Low-CPU Smooth RGB Engine & Preview:** Unified daemon RGB streaming (`rgb_engine_loop`) with `tokio::sync::Notify` zero-CPU idle parking (including `reactive`/`ripple` between keypresses), wall-clock `dt` pacing, stack-allocated `render_per_key_rgb`, precomputed `LampArray` slot mapping + dirty HID diffing (`R_RANGE`/`R_MULTI`), single-open `O_NONBLOCK` HID batching across Darfon/Primax/HP backends, and 40 FPS GUI RGB Studio preview with cached Cairo glyph extents and continuous speed-slider phase updates (`~59.8 µs/frame` / `<0.18%` of 1 CPU core at 30 FPS in `--release`).
- **Configurable AC / Battery Switching & Full AC Performance Restore (`#398`):** Added **Auto-Switch Profile on AC / Battery** toggle (`Settings -> Performance`, `omen-cli power auto-switch on|off`, `SetAcAutoSwitch` D-Bus), saved and restored the user's pre-battery profile on AC reconnect instead of overriding unconditionally, added CPU `energy_performance_preference` (EPP) & RAPL baseline restore on AC attach, added `power-profiles-daemon` `performance -> balanced` fallback, and updated `hp-wmi.c` so Victus-S `PL1/PL2` defaults are restored cleanly on both `Balanced` and `Performance`.

---

## Previously in v2.6.0

- **Saved Per-Key RGB Colour Presets (`#397`):** Save, load, overwrite, and delete named custom 104-key layouts (`~/.config/omenspace/key_presets.json`) directly in RGB Studio with live edit tracking, animation stop on preset load, and full TR/EN/ES localization (contributed by [@Dmitriy-Romanov](https://github.com/Dmitriy-Romanov)).
- **Ultra-Low-Latency Primax Reactive & Ripple RGB (`#385`):** Added instant `evdev` keypress wakeup to the daemon animation loop, single-open HID batching with dirty-page diffing (cutting USB bus traffic from 10 reports/frame down to 1–3 changed pages on `0461:4F03`), and full **Effect Speed** slider scaling for `Reactive` and `Ripple`.
- **Verified Power Profile Switching & UMA Diagnostics (`#396`):** Excluded `amd-pmf` from OMEN profile detection, added sysfs readback verification on `platform_profile` / `thermal_profile`, surfaced loud D-Bus errors with UMA (`gpu_mux_mode=3`) firmware guidance, and reverted GUI / tray / overlay active buttons automatically when hardware rejects a switch.
- **OMEN 16-wf1xxx (`8C76`/`8C77`/`8C78`) Thermal Stability & Anti-Oscillation (`#395`):** Eliminated `power-profiles-daemon` feedback oscillation and 1 Hz idle `pwm1_enable` / `platform_profile` rewrites; delegated `Auto` fan mode on `8C77`-family boards to native EC firmware (`prefer_ec_auto_fan`) + `hp-wmi` `PWM_MODE_AUTO` / `PWM_MODE_MAX` fixes so EC `0x06` never latches fans at 6000 RPM.
- **Instant Fan Dust Cleaning Stop Override (`#364`, `#385`) & Single-Zone Clamp (`#369`):** Added a **Stop & Restore Auto** cancel button (`CancelFanCleaning`) inside the Diagnostics cleaning modal and enforced `rgb_zone_count = 1` clamping in `omen-rgb-keyboard` hardware detection.

---

## Documentation & Security

- **[Changelog](CHANGELOG.md)** · **[Security Policy](SECURITY.md)**
- **[Architecture Map](docs/architecture.md)** · **[Backend Daemon](docs/backend.md)** · **[Kernel Driver](docs/driver.md)** · **[GUI](docs/gui.md)** · **[CLI](docs/cli.md)** · **[Contributing](docs/CONTRIBUTING.md)**

---

## Credits & License

Licensed under the **GPL-3.0 License**.

- **[yunusemreyl](https://github.com/yunusemreyl)** — Lead Developer
- **[tuxov](https://github.com/tuxov)** — Kernel Module Lead
- Community contributors: [@aloshy0](https://github.com/aloshy0), [@henfiber](https://github.com/henfiber), [@adrianatencia](https://github.com/adrianatencia), [@CodesRahul96](https://github.com/CodesRahul96), [@xcellsior](https://github.com/xcellsior), [@TitoTFP](https://github.com/TitoTFP), [@SafSaf0999](https://github.com/SafSaf0999), [@yijean34-source](https://github.com/yijean34-source), [@ManOfDiamond](https://github.com/ManOfDiamond), [@ASPRNG-PRGMR](https://github.com/ASPRNG-PRGMR), [@WoofahRayetCode](https://github.com/WoofahRayetCode), [@ktarxhun](https://github.com/ktarxhun), [@Dmitriy-Romanov](https://github.com/Dmitriy-Romanov), [@Megh-Rana](https://github.com/Megh-Rana), [@GarsGeekGM](https://github.com/GarsGeekGM), [@ShyVortex](https://github.com/ShyVortex), [@CrosserGamer](https://github.com/CrosserGamer), [@rayyanekaputra](https://github.com/rayyanekaputra), [@darmovys](https://github.com/darmovys), and [@elsinoro](https://github.com/elsinoro).

*Disclaimer: OMEN Space is an independent open-source project and is not affiliated with or endorsed by HP Inc.*
