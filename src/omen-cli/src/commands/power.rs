use anyhow::Result;
use clap::Subcommand;
use comfy_table::Table;
use omen_types::{MuxProxy, PowerProxy};
use zbus::Connection;

#[derive(Subcommand, Debug, Clone)]
pub enum PowerCommand {
    /// Set power profile (power-saver, balanced, performance)
    SetProfile { profile: String },
    /// Set CPU power limits PL1 & PL2 in Watts
    SetLimits {
        #[arg(long)]
        enabled: bool,
        #[arg(long)]
        pl1: i32,
        #[arg(long)]
        pl2: i32,
    },
    /// Apply CPU undervolt offset in mV (e.g. -50)
    Undervolt { mv: i32 },
    /// Set GPU Mux mode (hybrid, discrete, advanced, integrated)
    SetMux { mode: String },
    /// Enable or disable automatic AC/Battery Eco profile switching (on/off)
    AutoSwitch {
        #[arg(value_parser = ["on", "off", "true", "false"])]
        state: String,
    },
    /// Get current power profile and limits
    Info,
}

pub async fn handle(cmd: &PowerCommand, conn: &Connection) -> Result<()> {
    let proxy = PowerProxy::new(conn).await?;

    match cmd {
        PowerCommand::SetProfile { profile } => match proxy.set_power_profile(profile).await {
            Ok(res) if res.starts_with("FAIL") => {
                eprintln!("Failed to set power profile '{}': {}", profile, res);
                std::process::exit(1);
            }
            Ok(res) => println!("Response: {}", res),
            Err(e) => {
                eprintln!("Failed to set power profile '{}': {}", profile, e);
                std::process::exit(1);
            }
        },
        PowerCommand::SetLimits { enabled, pl1, pl2 } => {
            let res = proxy.set_power_limits(*enabled, *pl1, *pl2).await?;
            println!("Response: {}", res);
        }
        PowerCommand::Undervolt { mv } => {
            let res = proxy.set_undervolt(*mv).await?;
            println!("Response: {}", res);
        }
        PowerCommand::SetMux { mode } => {
            let mode_lower = mode.to_lowercase();
            if !matches!(
                mode_lower.as_str(),
                "hybrid" | "discrete" | "advanced" | "integrated"
            ) {
                eprintln!("{} {}", crate::i18n::t("mux_invalid"), mode);
                std::process::exit(1);
            }
            let mux_proxy = MuxProxy::new(conn).await?;
            let res = mux_proxy.set_gpu_mode(&mode_lower).await?;
            println!("Response: {}", res);
        }
        PowerCommand::AutoSwitch { state } => {
            let enabled = matches!(state.as_str(), "on" | "true");
            let res = proxy.set_ac_auto_switch(enabled).await?;
            println!("Response: {}", res);
        }
        PowerCommand::Info => {
            let res = proxy.get_power_profile().await?;
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
    }

    Ok(())
}
