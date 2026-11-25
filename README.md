<div align="center">
  <img src="images/omenspace.png" alt="OMEN Space Logo" width="110" />

  # OMEN Space

  **Native Linux hardware control center for HP OMEN, Victus & Transcend laptops.**  
  Built from the ground up in **Rust** with **GTK4 & Libadwaita** for near-zero overhead.

  <p>
    <a href="https://github.com/yunusemreyl/omen-space/releases/latest">
      <img src="https://img.shields.io/badge/Release-v2.5.0-e11d48?style=flat-square" alt="Release v2.5.0" />
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
| **RGB Studio** | 4-Zone OMEN, Single-Zone Victus, chassis Lightbar, Desktop RGB, and HID Per-Key RGB (Darfon `0d62:54bf` / `0d62:30bf` + Primax `0xFF13` McuSDK2) with daemon-hydrated preview, physical lamp calibration, EN/DE layout toggle, and low-CPU sysfs animations. |
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

## What's New in v2.5.0

- **Redesigned Performance Landing & Pinned Telemetry Dock:** Replaced the three floating telemetry cards with a full-width, boxless **CPU | GPU | Device Status** telemetry dock anchored to the bottom of the page; added bespoke custom SVG icons for every mode and fan control button, equal-height performance cards, a dedicated **+ New Preset** shortcut, and a full-width theme-adaptive custom fan curve studio.
- **Redesigned System Monitor:** Rebuilt **System Monitor** from scratch with a live top summary strip (**System Power · Power Profile · Fan Mode**), unified CPU & GPU panels featuring switchable 60-second **Power / Temp / Load** Cairo area charts, split **Memory & Storage** and **Battery & Power** panels, and a quiet hardware/firmware metadata footer.
- **Redesigned Quick Control HUD & RGB Hydration:** Rebuilt `omen-overlay` in native GNOME/Libadwaita styling with cross-device hotkey support, hydrated RGB Studio keyboard preview/swatch from live daemon color state with zero overhead, and updated workspace dependencies.

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
