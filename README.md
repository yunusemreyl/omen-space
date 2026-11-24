<div align="center">
  <img src="images/omenspace.png" alt="OMEN Space Logo" width="110" />

  # OMEN Space

  **Native Linux hardware control center for HP OMEN, Victus & Transcend laptops.**  
  Built from the ground up in **Rust** with **GTK4 & Libadwaita** for near-zero overhead.

  <p>
    <a href="https://github.com/yunusemreyl/omen-space/releases/latest">
      <img src="https://img.shields.io/badge/Release-v2.2.0-e11d48?style=flat-square" alt="Release v2.2.0" />
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
  <img src="images/perf.png" width="92%" alt="OMEN Space — Performance & Telemetry" />
</div>

---

## Overview

**OMEN Space** replaces bloated background utilities with a ~3 MB native Rust daemon and a responsive GTK4 desktop interface. It talks directly to HP WMI/ACPI interfaces, Embedded Controller (EC) registers, Intel RAPL / MSR, AMD Ryzen SMU, and USB HID lighting controllers over a privileged D-Bus service.

| Module | Capabilities |
| :--- | :--- |
| **Performance & Cooling** | `Eco`, `Balanced`, and `Performance` BIOS thermal profiles; software-smart `Auto` cooling, `Max` fan mode, custom multi-point fan curves with saved presets, one-click **Delegate to EC**, fan dust-cleaning mode, and continuous `>= 90°C` / `95°C` thermal protection. |
| **Advanced Power** | Real-time Intel RAPL `PL1` / `PL2` power limits (with boot-default tracking), `TCC Offset` temperature ceiling, MSR voltage offsets (with hardware `UV_LOCK` detection), GPU Dynamic Boost (`cTGP` / `PPAB`), and AMD Ryzen SMU tuning. |
| **Graphics Switcher (MUX)** | Switch between **Hybrid (MSHybrid)**, **Discrete (dGPU)**, and **Advanced Optimus** modes with live `eDP` display routing and NVIDIA driver telemetry. |
| **RGB Studio** | 4-Zone OMEN, Single-Zone Victus, chassis Lightbar, and Darfon Per-Key (`0d62:54bf`) RGB lighting with static, breathing, wave, and hardware MCU animations. |
| **Quick HUD (`Shift+F2`, configurable)** | In-game floating overlay and system tray applet for switching power and fan profiles on the fly without leaving your workflow. |
| **Profile Manager & Updater** | Automatic per-application performance rules (Wayland & X11) and an integrated **Update Center** with commit-level changelog tracking, live build progress, and `fwupd` (LVFS) firmware scanning. |

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
  <img src="images/keyboard.png" width="49%" alt="RGB Studio" />
  <img src="images/undervolt.png" width="49%" alt="Advanced Power & RAPL" />
</p>
<p align="center">
  <img src="images/mux.png" width="49%" alt="Graphics Switcher (MUX)" />
  <img src="images/appprofiles.png" width="49%" alt="Application Profile Manager" />
</p>
<p align="center">
  <img src="images/updater.png" width="49%" alt="Update Center" />
  <img src="images/diagnostic.png" width="49%" alt="System Monitor" />
</p>

<details>
<summary><b>More Views (Settings & Interactive CLI)</b></summary>
<br />
<p align="center">
  <img src="images/settings.png" width="49%" alt="Settings" />
  <img src="images/cli.png" width="49%" alt="Interactive CLI" />
</p>
</details>

---

## Architecture

| Component | Role |
| :--- | :--- |
| **`omen-space-daemon`** | Root `systemd` service managing WMI, ACPI, sysfs, EC, MSR/RAPL, and HID devices over D-Bus (`org.hp.omen.*`). |
| **`omen-gui`** | Native GTK4 + Libadwaita desktop application with scalable UI (`Ctrl+Shift+Plus` / `Minus`) and instant localization (**English**, **Türkçe**, **Español**). |
| **`omen-overlay`** | Lightweight Quick Access HUD (default `Shift+F2`, configurable in Settings) synchronized in real time with the daemon and main GUI. |
| **`omen-tray`** | StatusNotifierItem system tray applet for quick profile switching and background hotkey handling. |
| **`omen-cli`** | Scriptable command-line interface and interactive TUI (`omen-cli system info`, `omen-cli overlay toggle`, `omen-cli system check-update`). |
| **`hp-omen-extra`** | Custom DKMS kernel driver package (`hp-wmi` + `hp-omen-extra`) exposing fan PWM, thermal profiles, GPU TGP/PPAB, and RGB zones. |

---

## What's New in v2.2.0

- **Overhauled Update Center:** Commit-aware version tracking (`v2.2.0 • #sha`), automatic GitHub `main` & release comparison, inline step-by-step build progress with live log streaming, safe `pkexec` user toolchain resolution, and one-click app restart.
- **Expanded Hardware & Board Support:** Verified and added thermal/WMI quirks for **8EEC** (HyperX OMEN 15-gb0xxx + `WMBA` 512-byte ACPI buffer fix), **8EDD** (HyperX OMEN 16-ap0xxx), **8C2D** (Victus 15-fa1xxx), **8D26** (OMEN 16-xd0xxx), **8A17** (OMEN 17t-ck100), **8BCD** (OMEN 16-wf0xxx), **8A43**, **88D1**, **8902**, and **8EEF**.
- **Smarter Thermal Alerts & Notifications:** Continuous 3-second `>= 90°C` debounce filter eliminates false positives from brief boost spikes; user-session notifications replace in-place via `notify-send -p/-r` without stacking in GNOME Shell history.
- **UI & Telemetry Refinements:** Fixed custom fan curve preset selection and live graph preview, added **Reset Default** button and theme-proof color swatches in RGB Studio, added live sync to the `Shift+F2` Quick Overlay, and improved `UV_LOCK` / Core Ultra HX detection in Advanced Power.

---

## Credits & License

Licensed under the **GPL-3.0 License**.

- **[yunusemreyl](https://github.com/yunusemreyl)** — Lead Developer
- **[tuxov](https://github.com/tuxov)** — Kernel Module Lead
- Community contributors: [@aloshy0](https://github.com/aloshy0), [@henfiber](https://github.com/henfiber), [@adrianatencia](https://github.com/adrianatencia), [@CodesRahul96](https://github.com/CodesRahul96), [@xcellsior](https://github.com/xcellsior), [@TitoTFP](https://github.com/TitoTFP), [@SafSaf0999](https://github.com/SafSaf0999), [@yijean34-source](https://github.com/yijean34-source), [@ManOfDiamond](https://github.com/ManOfDiamond), [@ASPRNG-PRGMR](https://github.com/ASPRNG-PRGMR), [@WoofahRayetCode](https://github.com/WoofahRayetCode), and [@ktarxhun](https://github.com/ktarxhun).

*Disclaimer: OMEN Space is an independent open-source project and is not affiliated with or endorsed by HP Inc.*
