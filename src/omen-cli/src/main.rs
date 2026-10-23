use clap::{Parser, Subcommand};
use anyhow::Result;

mod dbus_proxy;
mod commands;
mod fetch;
mod i18n;

#[derive(Parser)]
#[command(name = "omen-cli")]
#[command(about = "Fastfetch-style CLI for Omen Space Daemon", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Show fastfetch / neofetch style OMEN system summary
    Fetch,
    /// Live battery charge, health, and status
    Battery,
    /// RGB keyboard lighting controls
    Rgb {
        #[command(subcommand)]
        cmd: commands::rgb::RgbCommand,
    },
    /// Fan control and modes
    Fan {
        #[command(subcommand)]
        cmd: commands::fan::FanCommand,
    },
    /// Power and performance options
    Power {
        #[command(subcommand)]
        cmd: commands::power::PowerCommand,
    },
    /// System and diagnostic operations
    System {
        #[command(subcommand)]
        cmd: commands::system::SystemCommand,
    },
    /// Quick HUD Overlay controls (Shift+F2)
    Overlay {
        #[command(subcommand)]
        cmd: commands::overlay::OverlayCommand,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let conn = if let Ok(sys_conn) = zbus::Connection::system().await {
        if let Ok(proxy) = dbus_proxy::PlatformProxy::new(&sys_conn).await {
            if proxy.ping().await.is_ok() {
                sys_conn
            } else if let Ok(sess_conn) = zbus::Connection::session().await {
                sess_conn
            } else {
                sys_conn
            }
        } else if let Ok(sess_conn) = zbus::Connection::session().await {
            sess_conn
        } else {
            sys_conn
        }
    } else {
        zbus::Connection::session().await?
    };

    match &cli.command {
        Some(cmd) => {
            run_command(cmd, &conn).await?;
        }
        None => {
            fetch::run_live_dashboard(&conn).await?;
        }
    }

    Ok(())
}

async fn run_command(cmd: &Commands, conn: &zbus::Connection) -> Result<()> {
    match cmd {
        Commands::Fetch => fetch::print_omen_fetch(conn).await?,
        Commands::Battery => commands::system::handle(&commands::system::SystemCommand::Battery, conn).await?,
        Commands::Rgb { cmd } => commands::rgb::handle(cmd, conn).await?,
        Commands::Fan { cmd } => commands::fan::handle(cmd, conn).await?,
        Commands::Power { cmd } => commands::power::handle(cmd, conn).await?,
        Commands::System { cmd } => commands::system::handle(cmd, conn).await?,
        Commands::Overlay { cmd } => commands::overlay::handle(cmd, conn).await?,
    }
    Ok(())
}
