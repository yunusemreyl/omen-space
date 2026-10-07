# Known Issues & Bug Reports

The following issues have been resolved and tested with the Omen Space 2.0 architecture updates and patches.

### Quick HUD Overlay (Shift+F2) does nothing, on any distro (#258)
- **Description:** Pressing Shift+F2 never opens the HUD, whether the main GUI is open or "closed" (the GUI is D-Bus activatable and may be running in the background). Reproduced on CachyOS (Arch) with KDE Plasma on Wayland.
- **Root cause:** `omen-gui` started `omen-overlay --daemon` at startup. The overlay has no such option, so it exited immediately, and because the GUI never called `wait()` on it the dead process stayed behind as a zombie. `omen-tray` decided whether the HUD was already open with `pgrep -x omen-overlay`, which also matches zombies, so it "closed" the corpse and returned without opening anything.
- **Status:** ✅ **Resolved.** The GUI no longer spawns the overlay; the tray checks `/proc` for a *live* process and logs what it does; the overlay closes only on Escape (the global Shift+F2 toggle is handled by the daemon/tray, and quitting on F2 as well raced with it); `omen-cli overlay daemon` is now a deprecated no-op. The Arch `PKGBUILD` and `flake.nix` now install `omen-overlay` as well (the PKGBUILD previously did not ship it at all), and `Cargo.lock` was refreshed so `cargo build --locked` works.
- **Known limitation:** the HUD is a normal window, not a layer-shell surface, so on Wayland it can appear behind a fullscreen game or without keyboard focus (KWin focus-stealing prevention). Use the tray menu entry or `omen-cli overlay toggle` as a fallback.

### [8A43] Bug Report — OMEN by HP Gaming Laptop 16-n0xxx #173
- **Description:** Power profiles return to balanced on auto seconds after changing. `hp-rgb-lighting` DKMS module fails to build on kernel 6.12.104 with Clang (`make LLVM=1`).
- **Status:** ✅ **Resolved.** Migrated from `hp-rgb-lighting` to the `hp-omen-extra` module, fixing Clang compilation errors. The power profile reset issue was resolved by decoupling `is_victus_s_thermal_profile`.

### [8912] Bug Report — OMEN by HP Laptop 16-c0xxx #171
- **Description:** Power profile, fan readout, fan mode and rgb lighting not working. Capabilities DB reports board 8912 not in database.
- **Status:** ✅ **Resolved.** Added the `8912` Board ID for OMEN 16-c0xxx to the `capabilities.rs` database.

### [8D41] Bug Report — OMEN MAX Gaming Laptop 16-ah0xxx #169
- **Description:** Changing RGB settings affects only RGB Bar. Zones are inverted horizontally (Zone 1 is on the right). Keyboard is breathing yellow/red, no per-key function active.
- **Status:** ✅ **Resolved.** Activated Per-Key support with the new Omen Space 2.0 HID backend and resolved the inverted zones via the `has_per_key_rgb: true` note in `capabilities.rs`.

### [88F7] Bug Report — OMEN by HP Laptop 17-ck0xxx #168
- **Description:** Keyboard lighting stays enabled after reboot even if the last action was to turn it off.
- **Status:** ✅ **Resolved.** Added a 5-second deferred apply mechanism in `rgb.rs` to prevent the Linux kernel from resetting the LED state during boot.

### [8D2F] Bug Report — OMEN Gaming Laptop 16-am0018nt #167
- **Description:** Only Auto and Max fan modes are available. Can't use performance/custom mode. Auto mode is too aggressive (starts at 40°C at 2000 RPM). `thermal_profile` node is missing.
- **Status:** ✅ **Resolved.** Activated the `force_fan_control_support` feature to provide a mandatory fallback to hwmon and `pwm1` privileges, enabling custom fan curves at the daemon level.

### [8C77] Bug Report — OMEN by HP Gaming Laptop 16-wf1xxx #157 / #162
- **Description:** Fan doesn't follow the custom curve and goes up to maximum RPM when CPU goes above 90°C.
- **Status:** ✅ **Resolved.** Added a toggle option (config) to the Omen Space 2.0 UI so the thermal protection mechanism (95°C Max Fan) can be disabled if desired.

### Add board 8D87 (OMEN MAX 16-ak0xxx, RTX 5080) #152
- **Description:** Needs patched hp-wmi for gpu_tgp/gpu_ppab since stock in-tree hp-wmi on kernel 7.0+ doesn't expose them. Missing from capabilities DB and exception list.
- **Status:** ✅ **Resolved.** Added the `8D87` Board ID to `capabilities.rs` and included it in the Kernel 7.0+ exception list in `driver/setup.sh` to ensure proper driver installation.

### [8BAA] Bug Report — OMEN by HP Gaming Laptop 16-wf0xxx #151
- **Description:** Fan RPM reading is always 0 even at max. No custom curve option available. Board not in database.
- **Status:** ✅ **Resolved.** Added the device to the `capabilities.rs` database. The EC access returning 0 issue was permanently fixed via the mandatory fan control patch over hwmon.

### [8C75] Bug Report — OMEN 17-db0xxx fans stuck at 0 RPM after overheat #175
- **Description:** Both fans silently stuck at 0 RPM after an overheat-induced reboot. `dmesg` showed repeated `ACPI Error: AE_AML_BUFFER_LIMIT` on `_SB.WMID.WMBX` and `_SB.WMID.WMBA`. `pwm1_enable` returned `0` (driver claims manual control), but all WMI fan-speed writes abort at the ACPI layer without error propagation. Fan recovers only at POST because the EC takes over during overheat.
- **Root Cause:** The OMEN 17-db0xxx ACPI table contains the same broken `GETB` helper as `8BAC` — a `CreateField` with zero length that causes `AE_AML_BUFFER_LIMIT` and aborts all `WMID` methods. Board `8C75` was not registered in the DMI table, so it fell through without the necessary no-EC workaround.
- **Status:** ✅ **Resolved.** Added `8C75` to `victus_s_thermal_profile_boards[]` in `driver/hp-wmi.c` with `omen_v1_no_ec_thermal_params` (matching the `8BAC` fix) to bypass EC thermal profile reads and prevent silent fan-write aborts. Also added `8C75` to `capabilities.rs` with `supports_fan_control_ec: false` and flagged it as `is_wmaa_abort_prone_board` so the daemon correctly reports degraded/ProfileOnly control instead of FullControl.

### [8A43] Bug Report — OMEN by HP Gaming Laptop 16-n0xxx #173
- **Description:** Power profiles return to balanced seconds after changing. DKMS build fails on Manjaro with kernel `6.12.104-1-MANJARO`:  installer attempted to install `linux61-headers` (6.1 LTS) instead of `linux612-headers` for the running kernel, so the custom `hp-wmi` never loaded and the thermal profile fix was never applied.
- **Root Cause:** The Arch/Manjaro header-detection block in `driver/setup.sh` only handled special suffixes (`-zen`, `-lts`, `-cachyos`, etc.). Vanilla Manjaro kernels (`-MANJARO`) fell through to the generic `linux-headers` meta-package, which resolves to the current LTS kernel's headers — mismatched with the running kernel.
- **Status:** ✅ **Resolved.** Added versioned-header probing to `driver/setup.sh`: for any unrecognised kernel suffix, the script now derives the Arch/Manjaro versioned package name from the running kernel's major.minor (e.g. `linux612-headers` for `6.12.x`) and verifies it exists in pacman's repos before falling back to the generic package. A clear warning is printed if the versioned package isn't found.

### Update Command removes system76-power without asking #156
- **Description:** Running `curl … | sudo bash` to update OmenCtl removed `system76-power` without prompting, because the installer pulled in `power-profiles-daemon` as a dependency which conflicts with `system76-power`.
- **Status:** ✅ **Resolved.** Added `check_conflicting_power_managers()` to `setup.sh` that runs **before** any package changes. If `system76-power` (or future conflicting tools) is detected, the user is shown a warning explaining the conflict and given the option to abort before the installer touches any packages.

### [8A43/8A44] Bug Report — OMEN Gaming Laptop 16-N0033DX unbootable after install on linux-cachyos-7.2.5 #211
- **Description:** After installing omen-space via the curl command, the system boots into emergency mode on kernel `linux-cachyos-7.2.5-1`. Booting with `linux-cachyos-lts` (6.18) works fine. Restoring a snapshot without omen-space installed also restores normal boot.
- **Root Cause:** The installer's stock `hp-wmi` override relied on renaming the stock `.ko` file to `.backup`. On CachyOS/Arch, `modinfo -n hp-wmi` can return the DKMS `updates/` path even before our module is installed, causing the backup step to silently skip. At boot, both the stock and custom `.ko` files are present; the kernel refuses to load the duplicate and drops to emergency mode. This affected all kernels ≥ 7.x where stock `hp-wmi` is shipped (LTS at 6.18 was unaffected because it ships without `hp-wmi`).
- **Status:** ✅ **Resolved.** Replaced the fragile file-backup mechanism with a `modprobe.d` blacklist (`/etc/modprobe.d/omen-space-hpwmi-override.conf`). The blacklist (`blacklist hp_wmi` + `install hp_wmi /bin/true`) prevents the stock module from loading at boot while the custom DKMS build in `updates/` takes priority. This also restores full feature parity on kernel 7.x — `gpu_tgp`, `gpu_ppab`, `chassis_temp`, and custom fan curves now work correctly. The `STOCK_FAN_SUPPORT`/`FORCE_CUSTOM_HPWMI` dual-mode logic (which previously degraded kernel 7.x users to RGB-only mode) was removed entirely.


---

## Active User Feedback / Open Issues (Omen Space 2.0)

### UI: Missing Application Icon
- **Description:** The application's icon appears as a red cross on the desktop/dock.
- **Status:** ⚠️ **Open.** Needs fixing in the `.desktop` entry or asset packaging.

### UX: Keyboard Color Menu is Confusing
- **Description:** Users struggle to find where to change the entire keyboard color because the bottom section focuses heavily on per-key settings. Changing all per-key colors unexpectedly changes the global backlight as well.
- **Status:** ⚠️ **Open.** Needs UX improvements in the RGB adjustment menu to clarify global vs. per-key assignments.

### Feature Regression: System Tray Support
- **Description:** Unlike `omenctl`, the new Omen Space interface doesn't minimize to the system tray on close.
- **Status:** ⚠️ **Open.** Needs to be re-implemented or properly integrated with the existing `omen-tray` backend.
