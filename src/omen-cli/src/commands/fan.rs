use anyhow::Result;
use clap::Subcommand;
use comfy_table::Table;
use omen_types::FanProxy;
use zbus::Connection;

#[derive(Subcommand, Debug, Clone)]
pub enum FanCommand {
    /// Set fan mode (auto, max, custom)
    SetMode { mode: String },
    /// Set target RPM for a specific fan
    SetTarget { fan_id: u32, rpm: u32 },
    /// Get current fan mode
    Mode,
    /// Get detailed fans info
    Info,
}

pub async fn handle(cmd: &FanCommand, conn: &Connection) -> Result<()> {
    let proxy = FanProxy::new(conn).await?;

    match cmd {
        FanCommand::SetMode { mode } => {
            let res = proxy.set_fan_mode(mode).await?;
            println!("Response: {}", res);
        }
        FanCommand::SetTarget { fan_id, rpm } => {
            let res = proxy.set_fan_target(*fan_id, *rpm).await?;
            println!("Response: {}", res);
        }
        FanCommand::Mode => {
            let res = proxy.get_fan_mode().await?;
            println!("Fan Mode: {}", res);
        }
        FanCommand::Info => {
            let res = proxy.get_fan_info().await?;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&res) {
                let mut table = Table::new();
                table.set_header(vec!["Property", "Value"]);

                if let Some(mode) = json.get("mode").and_then(|v| v.as_str()) {
                    table.add_row(vec!["Global Mode".to_string(), mode.to_string()]);
                }

                if let Some(fans) = json.get("fans").and_then(|f| f.as_object()) {
                    for (fan_id, details) in fans {
                        table.add_row(vec![
                            format!("Fan {}", fan_id),
                            format!(
                                "Current RPM: {} | Target: {} | Max: {}",
                                details.get("current").and_then(|v| v.as_u64()).unwrap_or(0),
                                details.get("target").and_then(|v| v.as_u64()).unwrap_or(0),
                                details.get("max").and_then(|v| v.as_u64()).unwrap_or(0)
                            ),
                        ]);
                    }
                }

                println!("{}", table);
            } else {
                println!("{}", res);
            }
        }
    }

    Ok(())
}
