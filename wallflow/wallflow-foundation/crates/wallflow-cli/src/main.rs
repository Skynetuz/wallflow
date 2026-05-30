use anyhow::Result;
use clap::{Parser, Subcommand};
use wallflow_config::{default_config_path, AppConfig};
use wallflow_core::CoreApp;
use wallflow_monitor::platform_monitor_provider;

#[derive(Debug, Parser)]
#[command(name = "wallflow")]
#[command(about = "WallFlow developer CLI")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print detected monitors as JSON.
    Monitors,

    /// Print the default config path.
    ConfigPath,

    /// Create default config if it does not exist.
    InitConfig,

    /// Run a minimal core smoke check.
    Smoke,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().init();
    let args = Args::parse();

    match args.command {
        Command::Monitors => {
            let provider = platform_monitor_provider();
            let monitors = provider.snapshot()?;
            println!("{}", serde_json::to_string_pretty(&monitors)?);
        }
        Command::ConfigPath => {
            println!("{}", default_config_path().display());
        }
        Command::InitConfig => {
            let path = default_config_path();
            let cfg = AppConfig::load_or_default(&path)?;
            cfg.save(&path)?;
            println!("wrote {}", path.display());
        }
        Command::Smoke => {
            let cfg = AppConfig::default();
            let app = CoreApp::new(cfg);
            let monitors = app.monitors()?;
            println!("core ok; monitors={}", monitors.len());
        }
    }

    Ok(())
}
