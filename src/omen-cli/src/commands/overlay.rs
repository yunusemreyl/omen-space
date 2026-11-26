use anyhow::Result;
use clap::Subcommand;
use crossterm::style::Stylize;
use omen_types::PlatformProxy;

#[derive(Subcommand, Debug, Clone)]
pub enum OverlayCommand {
    /// Toggle HUD overlay visibility (same as the global hotkey, Shift+F2 by default)
    Toggle,
    /// Deprecated: the overlay has no background mode and is started on demand
    #[command(hide = true)]
    Daemon,
}

pub async fn handle(cmd: &OverlayCommand, conn: &zbus::Connection) -> Result<()> {
    match cmd {
        OverlayCommand::Toggle => {
            let proxy = PlatformProxy::new(conn).await?;
            let res = proxy.toggle_overlay().await?;
            if res == "OK" {
                println!("{} Overlay toggle signal sent.", "✓".green().bold());
            } else {
                println!("{} Failed to toggle overlay: {}", "✗".red().bold(), res);
            }
        }
        OverlayCommand::Daemon => {
            // `omen-overlay` never implemented a `--daemon` flag: GApplication rejected
            // it and the process exited at once, leaving a zombie that blocked the
            // Shift+F2 toggle. The overlay is now opened on demand, so this is a no-op
            // kept only so existing scripts that call it do not break.
            println!(
                "{} `omen-cli overlay daemon` is deprecated and does nothing: the HUD is started on demand.\n  Use `omen-cli overlay toggle` or press the overlay hotkey (Shift+F2 by default).",
                "ℹ".cyan()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct Wrapper {
        #[command(subcommand)]
        cmd: OverlayCommand,
    }

    #[test]
    fn toggle_subcommand_parses() {
        let w = Wrapper::try_parse_from(["omen-cli", "toggle"]).unwrap();
        assert!(matches!(w.cmd, OverlayCommand::Toggle));
    }

    #[test]
    fn deprecated_daemon_subcommand_still_parses() {
        // Old scripts must keep working (as a no-op) after the fix.
        let w = Wrapper::try_parse_from(["omen-cli", "daemon"]).unwrap();
        assert!(matches!(w.cmd, OverlayCommand::Daemon));
    }
}
