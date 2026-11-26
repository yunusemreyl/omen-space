use anyhow::Result;
use clap::Subcommand;
use comfy_table::Table;
use omen_types::RgbProxy;
use zbus::Connection;

#[derive(Subcommand, Debug, Clone)]
pub enum RgbCommand {
    /// Set RGB animation mode and speed
    SetMode {
        /// Mode name (static, breathing, cycle, wave, rain, audio, etc.)
        mode: String,
        /// Animation speed (1 to 100)
        speed: i32,
    },
    /// Set static color for a specific zone (0 to 7, or 8 for all zones)
    SetColor {
        /// Zone ID (0 to 7, or 8 for all)
        zone: i32,
        /// Hex color code (e.g. FF0000)
        hex: String,
    },
    /// Set global power, brightness and direction
    SetGlobal {
        /// Power state (true: On, false: Off)
        #[arg(long)]
        power: bool,
        /// Brightness (0 to 100)
        #[arg(long)]
        brightness: i32,
        /// Direction (right, left, etc.)
        #[arg(long, default_value = "right")]
        direction: String,
    },
    /// Get current RGB configuration and state
    Config,
    /// Start the HID Per-Key RGB Calibration Wizard
    Wizard,
}

pub async fn handle(cmd: &RgbCommand, conn: &Connection) -> Result<()> {
    let proxy = RgbProxy::new(conn).await?;

    match cmd {
        RgbCommand::SetMode { mode, speed } => {
            let res = proxy.set_mode(mode, *speed).await?;
            println!("Response: {}", res);
        }
        RgbCommand::SetColor { zone, hex } => {
            let res = proxy.set_color(*zone, hex).await?;
            println!("Response: {}", res);
        }
        RgbCommand::SetGlobal {
            power,
            brightness,
            direction,
        } => {
            let res = proxy.set_global(*power, *brightness, direction).await?;
            println!("Response: {}", res);
        }
        RgbCommand::Config => {
            let res = proxy.get_state().await?;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&res) {
                let mut table = Table::new();
                table.set_header(vec!["Key", "Value"]);
                if let Some(obj) = json.as_object() {
                    for (k, v) in obj {
                        table.add_row(vec![k, &v.to_string()]);
                    }
                }
                println!("{}", table);
            } else {
                println!("{}", res);
            }
        }
        RgbCommand::Wizard => {
            let res = proxy.start_per_key_wizard().await?;
            println!("Wizard Started:\n{}", res);
            println!("Use daemon logs or check /tmp for further wizard instructions.");
        }
    }

    Ok(())
}
