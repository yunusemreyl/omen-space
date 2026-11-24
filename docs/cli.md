# OMEN Space CLI (`omen-cli`)

`omen-cli` provides a fast, unprivileged terminal CLI and interactive dashboard backed by `omen-space-daemon` over D-Bus.

## Common Usage Examples

```bash
# Inspect live telemetry, profiles, battery limit, and MUX status
omen-cli system info

# Check upstream updates
omen-cli system check-update

# Switch performance profile (power-saver | balanced | performance)
omen-cli power set-profile performance

# Set BIOS GPU MUX mode (hardware-supported modes only: hybrid | discrete | advanced | integrated)
omen-cli power set-mux hybrid

# Fan mode or manual speed percentage
omen-cli fan auto
omen-cli fan max
omen-cli fan 75

# Keyboard lighting
omen-cli rgb static FF0033

# Toggle Quick HUD overlay
omen-cli overlay toggle

# Interactive TUI dashboard
omen-cli
```
