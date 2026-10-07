# OMENSpace Tray (`omen-tray`)

The `omen-tray` crate provides a lightweight, unobtrusive system tray (AppIndicator/KDE System Notification Item) icon for OMENSpace. It is meant to run continuously in the background without eating up memory or CPU cycles.

## Responsibilities & Features

1. **Background Presence:**
   - Runs independently of the main GUI application. 
   - Uses the `ksni` crate to register itself seamlessly in the system tray of modern Desktop Environments like GNOME (via AppIndicator extension), KDE Plasma, XFCE, and Sway.
2. **Quick Actions Context Menu:**
   - **Launch GUI:** Fast shortcut to spawn the full `omen-gui` application.
   - **Performance Modes:** Right-click to quickly toggle between `Performance`, `Balanced`, and `Eco` modes.
   - **Fan Modes:** Quick toggle for `Auto` and `Max` fan modes.
   - **Exit:** Gracefully terminates the tray applet.
3. **D-Bus Integration:**
   - Just like `omen-gui` and `omen-cli`, the tray applet is entirely unprivileged.
   - It asynchronously sends `zbus` messages to the root `omen-space-daemon` over the D-Bus (`org.hp.omen.*` interfaces).

## Technical Architecture

### Core Libraries
- **`ksni`**: A Rust library that implements the StatusNotifierItem (SNI) protocol. This is the modern standard for Linux system trays, replacing legacy X11 embedded icons.
- **`zbus`**: Used to communicate with the DBus daemon.
- **`tokio`**: The asynchronous runtime. Even though the tray is a simple app, it uses `tokio` to asynchronously send DBus messages without blocking the GUI/Tray event loop.

### Code Breakdown (`src/main.rs`)

1. **`struct Tray`**: 
   The core structure implementing the `ksni::Tray` trait. It defines the icon (`omen-space`) and the title.
2. **`fn menu(&self)`**:
   This trait method constructs the actual drop-down menu hierarchy:
   - It uses `StandardItem` for clickable buttons and `SubMenu` for nested categories (e.g., "Güç Profili").
   - When an item is clicked, its `activate` closure is fired. For DBus calls, it spawns a detached `tokio::spawn` task so the tray menu closes instantly while the command executes in the background.
3. **`get_conn()`**:
   A helper function that attempts to connect to the DBus. It defaults to the System bus, but falls back to the Session bus if needed for testing.
4. **`set_power_profile()` & `set_fan_mode()`**:
   Helper methods that execute `conn.call_method()` targeting `/org/hp/omen/Power` and `/org/hp/omen/Fan` endpoints respectively.
5. **Main Event Loop (`main()`)**:
   Initializes the logger, creates the `ksni::TrayService`, spawns it, and then enters an infinite `tokio::time::sleep` loop to keep the process alive while the SNI server runs in a background thread.

## Quick HUD Overlay hotkey (Shift+F2)

The tray owns the lifecycle of the quick HUD (`omen-overlay`). The path from key press to window is:

1. `omen-space-daemon` (root) reads the keyboard through evdev. While either Shift key is held, a press of `KEY_F2` (code 60) is mapped to the name `overlay` by `hotkey_monitor::resolve_hotkey`.
2. The daemon broadcasts the D-Bus signal `org.hp.omen.Platform.MacroKeyPressed("overlay")`. `omen-cli overlay toggle` emits the very same signal, which makes it a handy way to test everything after the keyboard.
3. `omen-tray` is subscribed to that signal (and re-subscribes if the connection drops). It calls `spawn_overlay()`.
4. `spawn_overlay()` is a **toggle**: it looks for a *live* `omen-overlay` process of the current user (`overlay_proc::find_live_pids`, which reads `/proc` and ignores zombies). If one exists it is sent `SIGTERM`; otherwise a new one is started and reaped by a background thread.

Notes for debugging:

- Everything above only logs at `info` level. Both the tray and the daemon's hotkey monitor enable that by default, so `journalctl --user -u 'app-omenspace\x2dtray@autostart.service'` shows `Overlay hotkey detected, toggling overlay...` and `journalctl -u omen-space-daemon` shows `HotkeyMonitor: Detected Hotkey / Macro Key: overlay`.
- The HUD is started on demand. Nothing keeps it running in the background, and it has no `--daemon` mode.
- The tray must be running for the hotkey (and `omen-cli overlay toggle`) to do anything; the CLI reports `OK` even when no tray is listening.
- If your BIOS has *Action Keys Mode* enabled, the F-row sends brightness/media keys and you need to hold `Fn` (`Fn+Shift+F2`) to produce a real `F2`.
