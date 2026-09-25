# omen-kbd

Per-key RGB control for the HP OMEN MAX 16 keyboard (board 8D41) on Linux,
through the standard **HID LampArray** protocol. No kernel module, no DKMS,
no disabling Secure Boot, no dependencies outside the Python standard library.

Background and the road to the solution: [omen-max-8d41-keyboard-rgb.md](omen-max-8d41-keyboard-rgb.md).
Protocol specification: [BRIEF-omen-rgb-app.md](BRIEF-omen-rgb-app.md).

## Installation

```bash
bash packaging/install.sh                  # without reacting to keystrokes
bash packaging/install.sh --with-reactive   # with reaction to typing
```

No `sudo` up front — the script asks for the password itself, once. `--help`
prints the options, `--no-gui` skips the GUI and PySide6.

> **Install on the host, not inside a container.** `dnf`, `udev`, `systemd`
> and `/usr/lib/systemd/system-sleep` are host concerns, and the wrappers hard-
> code `/usr/bin/python3`, which is a different Python inside a distrobox. The
> installer detects a container on its own and refuses with a hint.

**The daemon is a system service** running as a separate, unprivileged
`omenkbd` user — for why, see **Permissions and privacy**. It comes up at
boot, so the keyboard is already lit on the login screen, with no linger
tricks needed. **The tray is a user unit**, because a tray icon without a
logged-in graphical session makes no sense.

| What | Where |
|---|---|
| code | `/usr/local/lib/omen-kbd/` |
| commands | `/usr/local/bin/omen-kbd{,-daemon,-gui}` |
| daemon service | `/etc/systemd/system/omen-kbd.service` |
| tray | `~/.config/systemd/user/omen-kbd-tray.service` |
| udev rules | `/etc/udev/rules.d/99-hp-lamparray{,-input}.rules` |
| wake hook | `/usr/lib/systemd/system-sleep/omen-kbd` |
| state and profiles | `/var/lib/omen-kbd` |
| lamp map cache | `/var/cache/omen-kbd` |
| socket | `/run/omen-kbd/omen-kbd.sock` |

The state, cache and runtime directories are provided by **systemd**
(`StateDirectory`, `CacheDirectory`, `RuntimeDirectory`), and the code reads
them from environment variables. Running the daemon by hand while working on
the code falls back to `~/.config`, `~/.cache` and `$XDG_RUNTIME_DIR` —
nothing to configure.

Re-running the installer is safe and doubles as the upgrade path. Adding
reactive typing later is just re-running it with `--with-reactive`.

> **Membership in the `omenkbd` group takes effect after you log back in.**
> Until then `omen-kbd` may not be able to reach the socket. The backlight
> itself works — the daemon doesn't depend on your session.

Uninstalling: `bash packaging/uninstall.sh`. It hands control back to the
firmware, removes the service, **both** udev rule files, the wake hook, the
system user and groups, and the program files. After that `/dev/hidraw*` and
`/dev/input/event*` go back to "root only". `--keep-config` leaves the
profiles, `--legacy` also cleans up traces of the old prototype.

## Permissions and privacy

The app asks for **two separate** access grants of very different weight.
The default install only grants the first.

| | Scope | When |
|---|---|---|
| **LEDs** | `/dev/hidraw*` of interface `04` — backlight control only | always |
| **Keystrokes** | 2 of ~43 input devices | only with `--with-reactive` |

### Why the daemon runs as a separate user

An effect that reacts to typing is **a keylogger by definition** — there is
no way to light a key under your finger without reading keystrokes. The
question is not whether this *can* read a password, but who gets to access it.

The obvious reflex fix — `TAG+="uaccess"` on the input devices — is **wrong**,
and not subtly so. `uaccess` grants an ACL to the **user**:

```
/dev/dri/card0    user::rw-  user:rj:rw-
```

That means every process running as you could read the keyboard. And on
Wayland the compositor deliberately forbids exactly this: an application gets
**only what you type into its own window**, nothing from other windows. A rule
using `uaccess` would reopen that hole, defeating a protection the system
already has.

That's why the daemon runs as `omenkbd`, and the input devices belong to the
`omenkbd-input` group, whose **only** member is that system user. Your account
is not in it. Applications talk to the daemon over a socket — they can change
colors without ever seeing the keys.

### Scope of the keystroke rule

The rule lives in a separate file, installed only on explicit request, and is
narrowed three times over. Checked live, what it actually catches:

```
event13   ACCESS  HP HP Gaming Keyboard II
event18   ACCESS  HP HP Gaming Keyboard II Keyboard
event14     —     HP HP Gaming Keyboard II Mouse
event15     —     HP HP Gaming Keyboard II Consumer Control
event17     —     HP HP Gaming Keyboard II Wireless Radio Control
event19     —     BY Tech Gaming Keyboard
event29     —     Compx 2.4G Wireless Receiver Keyboard
event266    —     Triadyn Nereid 2 Keyboard
```

`ID_USB_VENDOR_ID` + `ID_USB_MODEL_ID` filters out every other device, and
`ID_INPUT_KEYBOARD` filters out the pointing stick, media keys and radio
control of that same keyboard.

### Why the rules match on `ENV`, not `ATTRS`

Because the `ATTRS`-based version **does not work**, even though it circulates
in documentation and passes `udevadm verify`. In udev, every key in plural
form — `ATTRS`, `KERNELS`, `SUBSYSTEMS`, `DRIVERS` — must be satisfied by the
**same** parent device, and the needed attributes sit on two different ones:

```
.../usb3/3-9            idVendor, idProduct
.../usb3/3-9/3-9:1.4    bInterfaceNumber
```

Such a rule never matches: the group stays at its default, and the daemon
loops on `EACCES`. The `ID_USB_*` properties are set by the built-in `usb_id`
**on the device node itself**, so `ENV{}` doesn't have that limitation, and
`ID_USB_INTERFACE_NUM` replaces `bInterfaceNumber` with the same precision.

### Why `GROUP` and `MODE` aren't enough either

Because a POSIX ACL on the device node overrides them. On this machine an ACL
was left over from an earlier rule using `uaccess`:

```
user::rw-   user:rj:rw-   group::---   mask::rw-   other::---
```

`ls -l` then shows `crw-rw---- root omenkbd` and looks like the group has
access — but the `rw-` where the group belongs is the **ACL mask**, not the
group permission. The actual `group::` entry is empty, so a process in the
`omenkbd` group gets `EACCES`. Worse, it doesn't self-heal: `chmod` from a
udev rule changes exactly the mask, not the `group::` entry.

The rules therefore add an explicit ACL entry for the group, which works
independently of `group::` and survives later ACLs from logind:

```
RUN+="/usr/bin/setfacl -m g:omenkbd:rw $env{DEVNAME}"
```

### Verification by opening, not by group name

Both bugs above give the same symptom — `EACCES` in a loop — and the first
one passed a check based on `stat -c %G`, because the group **was** correct.
So the installer verifies differently: it **actually opens the device as the
daemon's user**:

```bash
runuser -u omenkbd -- python3 -c 'os.close(os.open(node, os.O_RDWR))'
```

On failure it aborts the install and diagnoses the cause: if the ACL has
`group::---`, it prints the full `getfacl` output and explains that the group
and mode are irrelevant; otherwise it points to `udevadm info` and `ausearch`
(SELinux).

The daemon's own error message makes the same distinction, so the log
immediately points at the cause, not just the symptom.

### Socket access vs. the current session

The socket has mode **0660** with the `omenkbd` group, and the
`/run/omen-kbd` directory has **0750** with the same group. The boundary is
therefore on the directory: without the group you can't even see that the
socket exists. A group member can change colors; that grants no access to
keystrokes, since those go straight to the daemon process and never leave
through the socket.

Groups are assigned **at login**, so right after installation the current
session doesn't have it yet and `omen-kbd` will report a permission error.
The backlight still works — the daemon is a system service and doesn't depend
on your session. Without logging out:

```bash
sg omenkbd -c 'omen-kbd status'
```

The installer tells this state apart from a real failure: it checks the
daemon as root, and reports a missing group in the current session as
information, not an error.

### No side channel through the socket

The socket is reachable from your account, so **no command may leak lamp
state** — from which lamp is lit you could infer what was just pressed. This
is an invariant guarded by tests: `test_resilience.py` takes the actual frame
and searches for its values in the responses of every read command.

### Remaining layers

The daemon unit has `PrivateNetwork=yes` and
`RestrictAddressFamilies=AF_UNIX`, so the process **cannot open a network
socket** at all. On top of that: `ProtectSystem=strict`, `ProtectHome=yes`,
`DevicePolicy=closed` with an explicit allow-list of device classes,
`MemoryDenyWriteExecute` and `NoNewPrivileges`. There are no external
dependencies: plain Python from the standard library plus PySide6 from
Fedora's repository, zero packages from pip.

Checking who has access to the keys:

```bash
getent group omenkbd-input
```

> **Also check the `input` group.** If your account is a member, the whole
> isolation above is moot — the `input` group grants access to **every**
> input device on the system, to every one of your processes. `omen-kbd`
> doesn't use it and will never add you to it, but it could have been left
> behind by another tool:
>
> ```bash
> id -nG | tr ' ' '\n' | grep -x input && echo "you are in the input group"
> ```
>
> Removing yourself: `sudo gpasswd -d $USER input`, then log back in.

Revoking just this access, without uninstalling everything:

```bash
sudo rm /etc/udev/rules.d/99-hp-lamparray-input.rules && sudo udevadm control --reload-rules && sudo udevadm trigger --action=add --subsystem-match=input
```

## GUI

`omen-kbd-gui`, a menu entry named "OMEN Keyboard", a tray icon.

The window shows a **clickable keyboard layout drawn from the firmware's real
XY coordinates** — there is no artwork or hardcoded layout here, so it works
the same way on ISO, ANSI, and numpad-less variants. Lamps belonging to one
key (Space has 5, LShift has 3) are merged into a single rectangle, so you
click keys, not individual diodes.

The animation preview is computed **locally, with the same effects module**
the daemon uses — it's fully independent of the hardware, so the window shows
what's actually on the keyboard without polling the daemon 30 times a second.
The timer only runs while the window is visible and the effect is animated.

Per-key mode: click selects a key, Ctrl adds more, dragging paints the
selection, a button applies the brush color.

**The "Control: BIOS / App" switch** shows who is currently in charge of the
backlight. This is a state, not an action, hence a switch rather than a
"release" button. `BIOS` means the keyboard's firmware plays its own effect
(the factory yellow-orange pulse) and the app sends nothing — mode settings
are then grayed out, since they wouldn't change anything anyway. `App` takes
control back and returns to the selected mode. Changing the mode implicitly
gives control back to the app.

Closing the window hides it to the tray. Quitting the GUI **does not turn off
the backlight** — the daemon lives independently. Turning it off is what the
"Off" mode is for; handing back the hardware is what the switch to `BIOS`
is for.

## Language

The app is bilingual: Polish and English, switchable per user. Documentation
(this file and the others in the repository) is English-only; the
application interface — GUI, tray, CLI — ships both.

```bash
omen-kbd --lang en status      # one-off override for this invocation
omen-kbd lang en               # save the preference for next time
```

In the GUI: tray menu → Language. Precedence, highest first: an explicit
`--lang`/tray choice, the `OMEN_KBD_LANG` environment variable, the saved
preference (`~/.config/omen-kbd/language`), the system locale
(`LC_ALL`/`LC_MESSAGES`/`LANG`) — Polish only if that locale explicitly says
so, English otherwise. `install.sh`/`uninstall.sh` follow the same rule
independently (`packaging/i18n.sh`), including their own `--lang` flag; since
`sudo` resets the environment, the user-facing half of each script resolves
the language once and passes it explicitly to the root-privileged half.

Everything a parameter can be **saved as** — axis direction, decay curve,
scanner motion — is stored as a language-neutral code (`x`/`y`/`d`,
`soft`/`linear`, `bounce`/`loop`), never as translated text. Switching
language never breaks an existing profile; a completeness test
(`test/test_i18n.py`) checks every effect, every parameter and every choice
value in the engine has a translation in both languages, so a newly added
mode can't silently ship half-translated.

The daemon's own diagnostic messages (permission errors, ACL/udev
troubleshooting) are not translated — they're aimed at whoever is debugging
the install, and stay in the language they were written in for grep-ability.

## Lighting modes

| Mode | | What it does |
|---|---|---|
| `static` | Solid color | one color across the whole keyboard |
| `gradient` | Gradient | a blend between two colors along an axis |
| `spectrum` | Spectrum cycle | the whole keyboard cycles through the spectrum |
| `wave` | Rainbow wave | a rainbow sliding along an axis |
| `aurora` | Aurora | three incommensurate waves blending two colors |
| `plasma` | Plasma | an organic, flowing full-spectrum pattern |
| `wheel` | Rainbow wheel | full spectrum arranged by angle around a point, rotating |
| `breathe` | Breathe | pulsing in one color |
| `ripple` | Ripple | concentric waves from a chosen point |
| `scanner` | Scanner | a streak sweeping back and forth or looping |
| `twinkle` | Twinkle | random sparkles, each key with its own phase |
| `confetti` | Confetti | like Twinkle, but each sparkle picks its own random hue |
| `fire` | Fire | flicker climbing from the bottom rows upward |
| `rain` | Rain | drops falling in columns, each with a trail |
| `perkey` | Per-key | keys painted individually by hand |
| `off` | Off | dark, but the host still holds control |

Every mode has its own parameters — the full list with ranges and defaults:
`omen-kbd effects`.

## Reactive typing

A key flashes under your finger and fades — it works **on all 16 modes at
once**, because the overlay renders after the base layer, blending its color
into whatever is already in the frame. Nothing about the mode itself needs to
change for this to work.

Requires installing with `--with-reactive` — see **Permissions and privacy**
above. Without it the switch can still be turned on, but the status shows
`available: false` with a hint on what to do; no key will actually light up.

```bash
omen-kbd reactive on
omen-kbd reactive set color=00FF88 decay=0.4 curve=linear
omen-kbd reactive status
omen-kbd reactive off
```

| Parameter | Meaning |
|---|---|
| `color` | flash color |
| `decay` | decay time in seconds (0.05–3.0) |
| `curve` | `soft` (smooth fade-out) or `linear` |
| `intensity` | flash strength (0.05–1.0), independent of the global brightness slider |

In the GUI it's a collapsible "React to keystrokes" section under the mode
picker; in the tray, a checked item in the main menu.

### Why it works on every mode without touching effect code

The effects engine has no idea reactive typing exists. The daemon renders the
base frame exactly as always (`effect.render(...)`), and **then** — if there
are any fresh keystrokes — the overlay blends its color in, with a fade that
depends only on elapsed time. Adding a seventeenth mode to `effects.py`
automatically inherits the same reactive behavior, with zero lines changed.

A static effect normally never schedules a frame — that's where the zero idle
CPU usage comes from. But while a flash is fading, the frame schedule turns on
for the duration of the fade, even with a static mode selected; otherwise the
flash would light up and stay lit forever.

### Mapping a physical key to a lamp

```
physical key → Linux evdev code → HID Usage (0x07) → lamp_id (from the firmware)
```

The evdev→HID table (`omenkbd/core/evdev_map.py`) is the same one the Linux
kernel uses to translate HID reports into evdev events — so it matches
exactly what the keyboard actually sends. The last step, HID Usage→lamp_id,
uses the `InputBinding` read from the firmware while building the map, so it
works the same way on every keyboard variant. One key (e.g. Space) sometimes
maps to several lamps — the overlay lights all of them at once.

Events are read **only** from the built-in keyboard: the filter is the same
VID:PID as the LampArray device already being controlled. External keyboards
(USB, Bluetooth) won't light up anything here — reactive typing only responds
to what you actually type on the laptop.

### The Copilot key sends phantom Shift and Windows presses

On this keyboard (and likely other recent laptops with an AI/Copilot key),
the key has no HID Keyboard-page usage of its own. Instead the firmware
emulates it with a compatibility shortcut left over from before operating
systems had native support for it: pressing Copilot alone sends real
`KEY_LEFTMETA`, `KEY_LEFTSHIFT` and `KEY_F23` events, verified empirically —
one press produces exactly the evdev burst `[125, 42, 193]`. Without
filtering, reactive typing would light the Left Shift and Left Meta lamps
instead of nothing, which is misleading since neither was actually pressed.

`KEY_F23` (193) is the tell: this keyboard has no physical F23 key, so that
code only ever appears as part of this shortcut. `filter_copilot_burst()`
(`omenkbd/core/inputwatch.py`) strips Shift and Meta from a batch of events
whenever F23 is present in the same batch, and leaves every other batch —
including a real, standalone Shift or Meta press — untouched.

## Usage

```bash
omen-kbd status                          # what's lit right now
omen-kbd all 00FF88                      # a solid color
omen-kbd all teal -b 120                 # color by name, brightness 0-255
omen-kbd gradient red blue --axis x      # gradient along axis x, y or d
omen-kbd wave --speed 0.3                # rainbow
omen-kbd breathe teal --period 6         # pulsing
omen-kbd effect fire speed=3 height=0.9  # any mode plus its parameters
omen-kbd effect rain color=00C2FF density=12
omen-kbd effects                         # catalogue of modes with parameters
omen-kbd keys W,A,S,D,Space red --base 101010
omen-kbd preset gaming                   # gaming, typing, wasd, mods
omen-kbd brightness 60
omen-kbd off                             # turn off (host still holds control)
omen-kbd control bios                    # let the firmware drive (factory pulse)
omen-kbd control app                     # the app drives
omen-kbd reactive on                     # key flashes under your finger
omen-kbd lang en                         # switch the interface language

omen-kbd profile save night
omen-kbd profile list
omen-kbd profile load night

omen-kbd map                             # lamp map read from the firmware
omen-kbd keylist                         # key name -> lamp id
```

Settings save themselves on every change and come back after a restart or
after waking from sleep.

## Architecture

```
        as YOU (rj)                          as omenkbd (system service)
┌─────────────┐  ┌──────┐  ┌──────────┐
│  GUI + tray │  │ CLI  │  │sleep hook│
└──────┬──────┘  └──┬───┘  └────┬─────┘
       └──── unix socket ───────┘        ┌──────────────────────┐
                    └───────────────────►│    omen-kbd daemon   │
              /run/omen-kbd/…sock        │  30 fps, layers      │
              group omenkbd, 0660        └──────┬────────┬──────┘
                                                │        │
                          ioctl HIDIOC(S|G)FEATURE│        │ read (only with --with-reactive)
                                                ▼        ▼
                                       /dev/hidrawN   /dev/input/event13,18
                                       group omenkbd  group omenkbd-input
                                                      ← you are NOT here
```

The permission boundary runs through the middle of the diagram. Your
processes only ever reach the socket; the keystroke stream never crosses it.

**The daemon is the only writer.** A frame is a dozen or so `LampMultiUpdate`
reports with `LampUpdateComplete=0` on all but the last; two processes writing
at once would interleave and break the framing. The client sends a single
line of JSON to an already-running process, so `omen-kbd all 00FF88` takes
about 40 ms including interpreter startup.

| Module | Role |
|---|---|
| `omenkbd/core/device.py` | discovery by descriptor, reports 1–6, `ioctl` |
| `omenkbd/core/layout.py` | lamp map from the firmware, cache, addressing by key |
| `omenkbd/core/hidkeys.py` | HID Usage ↔ key name |
| `omenkbd/core/color.py` | color parsing, HSV, rainbow table |
| `omenkbd/engine/frame.py` | frame buffer, differential sending, packet framing |
| `omenkbd/engine/effects.py` | 16 modes plus their parameter declarations |
| `omenkbd/engine/reactive.py` | overlay: flash decay, independent of mode |
| `omenkbd/core/evdev_map.py` | evdev keycode ↔ HID Usage |
| `omenkbd/core/inputwatch.py` | reads `/dev/input/event*`, event parser |
| `omenkbd/engine/state.py` | state persistence and profiles |
| `omenkbd/engine/daemon.py` | frame loop, socket server, reconnection |
| `omenkbd/client.py` | socket client, shared by CLI and GUI |
| `omenkbd/cli.py` | command-line interface |
| `omenkbd/i18n.py` | bilingual UI strings, single source of truth |
| `packaging/i18n.sh` | the same, for the install/uninstall shell scripts |
| `omenkbd/gui/keyboard.py` | keyboard canvas from real geometry |
| `omenkbd/gui/panels.py` | per-effect parameter controls |
| `omenkbd/gui/window.py` | main window with a live preview |
| `omenkbd/gui/tray.py` | tray icon, quick profiles |

### Decisions worth knowing before changing the code

**The schedule is absolute, not `sleep(1/30)`.** `next += interval`, and on a
missed frame it jumps to the nearest future slot instead of catching up with a
burst. Over an hour of animation the drift stays under 0.2%.

**A static effect has no deadline** — the selector blocks indefinitely and the
daemon uses exactly zero CPU. That matters on a laptop.

**Sending is differential.** An unchanged frame means zero `ioctl` calls. A
uniform frame means one `LampRangeUpdate` instead of fifteen
`LampMultiUpdate`s.

**`AutonomousMode=0` is re-asserted on every frame.** The firmware reverts to
its own effect after S3 and then **silently ignores** colors from the host —
writes go through without error, they just do nothing. One extra `ioctl` per
frame removes this entire class of failure.

**Report 2 is ignored by the firmware.** The controller keeps its own cursor,
incrementing it on every read of Report 3 regardless of what was actually
asked for, wrapping around at the end and **surviving across processes**.
That's why the map is read sequentially, trusting the `LampId` field in the
response until a full set has been collected.

**The map is never hardcoded.** Geometry and key assignments come from the
firmware, so gradients, waves and presets work on every keyboard variant
(ISO/ANSI, numpad-less versions) with no code changes.

**Effects declare their own parameters, and the GUI builds controls from
that.** Every effect has `PARAMS` with a type, range, step and default value;
`catalogue()` sends this over the socket, and the window assembles sliders and
dropdowns automatically. Adding a mode requires touching zero lines of
interface code, and the engine and GUI can never drift apart on ranges —
panel tests guard against that.

**Animated effects are stateless — a frame depends only on `t`.** Rain
doesn't keep a list of drops, it computes which ones would be alive at a given
moment: a drop's index determines its start time and column. Fire and
Twinkle use a hashed noise (FNV-1a), not `random`. As a result nothing drifts
after waking from sleep, restarting the daemon never loses phase, and the GUI
preview shows exactly what the keyboard shows. There's a test for that.

**Canvas geometry measures spacing within a row, not across the whole
table.** Rows are offset relative to each other, so the union of all X
coordinates has random ~1 mm gaps at a real spacing of 18 mm — a naive minimum
drew the whole keyboard as slivers. The median spacing within each row is used
instead, and a cell reaches halfway to its neighbor, never further than half
the spacing.

**The firmware can return two lamps at the same coordinate.** On this
keyboard, Up and Down both sit at `(237000, 99000)`, because physically
they're a half-height stack and LampArray only knows one point. The shared
cell is split vertically — without that, one would render on top of the
other and become unclickable.

**Signals go through a self-pipe.** Without it, `SIGTERM` sets a flag, but
`select()` resumes automatically (PEP 475) and the loop never checks the
condition — the daemon hangs until `SIGKILL`.

## Tests

```bash
python3 test/test_omenkbd.py       # 49 tests — protocol, effects, sending
python3 test/test_resilience.py    # 48 tests — resilience, permissions, reactive, no side channel
python3 test/test_reactive.py      # 32 tests — evdev↔HID, event parser, flash decay, Copilot key filter
python3 test/test_gui_geometry.py  # 15 tests — canvas and panels (skipped without PySide6)
python3 test/test_i18n.py          # 24 tests — translation completeness, language-neutral state
bash    test/smoke.sh 60           # on real hardware
```

Tests without a device swap in a stand-in for the HID layer that records
every report — they check packet framing, differential sending, reconnection
after the device disappears, `hidraw` renumbering, waking from sleep, and
state persistence. Canvas tests guard invariants that are easy to break: every
lamp has its own element, is clickable, and doesn't overlap a neighbor.
i18n tests check that every effect, every parameter and every choice value in
the engine has a translation in both languages, and that nothing saved to
state or a profile is language-dependent text.

## Status

Done: device layer, cached map, a 16-mode engine, reactive typing, profiles,
persistence, daemon, CLI, a GUI with a per-key editor and tray, bilingual
interface, install and uninstall.

Not done: an RPM package (stage 5).

## Limitations

* The chassis LED strip is a **separate channel** (WMI `0x020009`), not
  handled here. Projects like `omen-rgb-keyboard`, `hp_rgb_lighting` and
  similar drive that strip, not the keyboard.
* OpenRGB won't detect this keyboard — there's no generic LampArray driver.
* The lamp map depends on the specific unit. The one in this repository is
  test data.
