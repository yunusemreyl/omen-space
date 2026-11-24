# Kernel Driver (`hp-wmi` & `hp-omen-extra` DKMS)

The [`driver/`](../driver) directory builds two Linux kernel modules via DKMS:

- **`hp-wmi`**: Enhanced HP platform WMI driver exposing laptop thermal profiles, MUX capabilities/switching, battery charge mode (`0x2B`), chassis sensor, and GPU TGP/PPAB knobs.
- **`hp-omen-extra`**: Companion driver exposing 4-zone/lightbar RGB sysfs controls and hardware interfaces where required.

## Sysfs Interfaces Exposed (`hp-wmi`)

| Sysfs Path | Access | Description |
| :--- | :--- | :--- |
| `/sys/devices/platform/hp-wmi/gpu_mux_mode` | RW | BIOS GPU MUX selector (`0=hybrid`, `1=discrete`, `2=advanced`, `3=integrated`). Verified against hardware mask before writes. |
| `/sys/devices/platform/hp-wmi/gpu_mux_supported_modes` | RO | Space-separated list of modes advertised by BIOS `HPWMI_GET_SYSTEM_DESIGN_DATA` (e.g. `hybrid discrete` or `hybrid discrete advanced`). |
| `/sys/devices/platform/hp-wmi/battery_charge_mode` | RW | Legacy HP WMI `0x2B` (`GBCC`/`SBCC`) charge control (`auto`, `inhibit`, `hold`); unsafe force-discharge/ship commands are rejected. |
| `/sys/devices/platform/hp-wmi/gpu_tgp` / `gpu_ppab` | RW | GPU Dynamic Boost / Total Graphics Power toggles on supported Victus S / OMEN systems. |
| `/sys/devices/platform/hp-wmi/chassis_temp` | RO | Firmware WMI `0x23` board surface sensor (with WMBA-abort board guards). |
| `/sys/devices/platform/hp-wmi/hwmon/hwmon*/fan*_target` | RW | Fan speed targets & live RPM telemetry. |

## DKMS & Secure Boot Notes

- `driver/dkms.conf` automatically detects whether the running kernel was compiled with **Clang/LLVM** (`/proc/version`) and passes `LLVM=1` when required (e.g., CachyOS / LLVM kernels).
- `setup.sh` and `driver/setup.sh` support automatic Secure Boot MOK signing (reusing distro `shim-signed` keys where present) and kernel-compatible `.ko.xz` / `.ko.zst` compression.
