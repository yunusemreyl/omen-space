# Security Policy

## Supported Versions

| Version | Supported |
| :--- | :--- |
| `2.5.x` (latest release on `main`) | Yes |
| `< 2.5.0` | Security fixes only backported upon request; please upgrade to `2.5.x` |

---

## Reporting a Vulnerability

If you discover a security vulnerability (for example: privilege escalation across D-Bus or Polkit, unsafe sysfs/WMI writes, shell injection in installers or daemon helpers, or unsafe driver memory access), **please do not open a public issue**.

Report it privately via GitHub Security Advisories:

- **Private Security Advisory:** https://github.com/yunusemreyl/omen-space/security/advisories/new

Include as much detail as possible:
- Affected component (`omen-space-daemon`, `omen-gui`, `omen-cli`, `omen-tray`, `omen-overlay`, `driver/hp-wmi.c`, or `setup.sh`)
- Laptop model & DMI Board ID (`/sys/class/dmi/id/board_name`) if hardware-specific
- Reproduction steps or proof of concept

You can expect an initial response within 72 hours and a coordinated fix/disclosure once a patch is verified.

---

## Security Architecture

OMEN Space follows a strict **split-privilege architecture**:

1. **Privileged Daemon (`omen-space-daemon`):**
   - Runs as `root` under `systemd` (`data/omen-space-daemon.service`) or `OpenRC` (`data/omen-space-daemon.openrc`).
   - Performs all direct hardware I/O (`hp-wmi` / `hp-omen-extra` sysfs, hwmon, Intel RAPL/MSR, AMD Ryzen SMU, NVML, and USB HID controllers).
   - Validates and clamps all incoming numeric inputs (fan speeds, RAPL PL1/PL2 power limits, TCC offsets, battery stop/start hysteresis thresholds, and MUX mode allowlists) rather than trusting client strings or invoking raw shells from D-Bus inputs.

2. **Unprivileged Clients (`omen-gui`, `omen-cli`, `omen-tray`, `omen-overlay`):**
   - Run exclusively with standard desktop user privileges (`never` as root).
   - Communicate with the daemon over the Linux System Bus (`org.hp.omen.*`).

3. **D-Bus & Device Access Policy (`data/org.hp.omen.conf`, `data/99-omen-space.rules`):**
   - Ownership of `org.hp.omen` on the system D-Bus is restricted strictly to `root`.
   - Method calls to `org.hp.omen` are restricted by D-Bus policy to `root` and members of the `omen-hw`, `wheel`, or `adm` groups.

4. **Kernel Driver Guards (`driver/hp-wmi.c`, `driver/hp-omen-extra.c`):**
   - `battery_charge_mode` enforces a strict write whitelist (`auto`, `inhibit`, `hold`) and intentionally refuses firmware force-discharge (`0x0200`) and shipping-mode (`0x0600` / `0x0700`) commands.
   - `gpu_mux_mode_store` verifies every requested mode against the BIOS capability bitmask returned by `hp_wmi_get_mux_supported_modes()` (`gpu_mux_supported_modes`) before issuing WMI writes, and guards non-NVIDIA AMD Advantage systems against unsafe MUX WMI queries.
   - Known WMAA/WMBA abort-prone boards are guarded against unsafe init-time or EC/WMI thermal probes.

5. **Secure Boot DKMS Signing (`setup.sh`, `driver/setup.sh`):**
   - When Secure Boot is enabled, the installer reuses the distribution `shim-signed` MOK key when available or generates a local MOK key (`mokutil --import`) and strips/recompresses `.ko.xz` / `.ko.zst` modules compatibly with Linux kernel module signing.
