mod commands;
mod config;
mod icons;
mod notifier;
mod parser;
mod scheduler;
mod state;
mod tray;

use std::path::PathBuf;

use clap::Parser;
use log::{error, info};

use crate::commands::perform_check;
use crate::config::{CliOverrides, load_config};

#[derive(Debug, Parser)]
#[command(name = "pactrack", version, about = "Arch package update tray tracker")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,

    #[arg(long)]
    poll_minutes: Option<u64>,

    #[arg(long)]
    no_aur: bool,

    #[arg(long)]
    once: bool,
}

fn main() {
    env_logger::init();

    let cli = Cli::parse();
    let overrides = CliOverrides {
        poll_minutes: cli.poll_minutes,
        no_aur: cli.no_aur,
    };

    let (config, config_path) = match load_config(cli.config, &overrides) {
        Ok(v) => v,
        Err(err) => {
            error!("{err}");
            std::process::exit(2);
        }
    };

    info!("using config path: {}", config_path.display());

    if cli.once {
        match perform_check(&config) {
            Ok(result) => {
                println!("official updates: {}", result.snapshot.official.len());
                println!("aur updates: {}", result.snapshot.aur.len());
                println!("total updates: {}", result.snapshot.total_count());
                if let Some(helper) = result.helper {
                    println!("detected aur helper: {helper}");
                }
            }
            Err(err) => {
                error!("one-shot check failed: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    if let Err(err) = tray::run(config) {
        error!("failed to start tray app: {err}");
        std::process::exit(1);
    }
}
