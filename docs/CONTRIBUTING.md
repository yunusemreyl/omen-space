# Contributing to OMEN Space

Thank you for contributing to **OMEN Space**! From board verification reports to kernel driver fixes and UI polish, pull requests are welcome.

## Development Environment

1. **Prerequisites:**
   - Rust stable toolchain (`rustup` with `clippy` and `rustfmt`)
   - `libgtk-4-dev`, `libadwaita-1-dev`, `libdbus-1-dev`, `libudev-dev`, `libhidapi-dev`, `pkg-config`
   - Linux kernel headers (if editing `driver/`)

2. **Local Build, Test & Lint:**
   ```bash
   cargo fmt --all -- --check
   cargo check --workspace --locked
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   (For kernel driver changes, also build modules with `make -C driver` and check `git status` after `make -C driver clean`.)

## Architectural Rules

1. **Split Privileges ([SECURITY.md](../SECURITY.md)):**
   - `omen-gui`, `omen-cli`, `omen-tray`, and `omen-overlay` must **never** require `sudo`.
   - All hardware I/O (`/sys/`, `/dev/hidraw*`, MSR, EC) belongs strictly in `omen-space-daemon` or `driver/` and must validate/clamp inputs.
2. **D-Bus & Hardware Gating:**
   - Check `data/org.hp.omen.conf` when adding D-Bus interfaces.
   - Gate optional BIOS/hardware features (`gpu_mux_supported_modes`, `battery_charge_mode`, Per-Key PIDs) gracefully so unsupported laptops never show broken controls.
3. **Never Block GTK Main Thread:**
   - Dispatch D-Bus/process work via `glib::spawn_future_local` + `tokio` runtime handles instead of blocking GTK threads.
4. **Localization:**
   - Update **TR**, **EN**, and **ES** strings in `src/omen-gui/src/i18n/{en,tr,es}.rs`, escaping literal `&` as `&amp;` for Adwaita markup rows/groups.

## Submitting a Pull Request

Please follow `.github/PULL_REQUEST_TEMPLATE.md` and review [CODE_REVIEW_GUIDE.md](CODE_REVIEW_GUIDE.md) before opening a PR against `main`.
