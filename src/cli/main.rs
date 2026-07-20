mod args;
mod commands;
mod config_loader;
mod coordinator;
mod menu;
mod output;
mod progress;
mod state;

use std::process::ExitCode;
use clap::Parser;
use args::{Cli, Command};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // version and init don't need an existing config
    match &cli.command {
        Some(Command::Version) => return commands::version::run(),
        Some(Command::Init)    => return commands::init::run().await,
        _ => {}
    }

    let cfg = match config_loader::load_config(cli.config).await {
        Ok(c)  => c,
        Err(e) => {
            output::error(&format!("{e}"));
            menu::no_config_error();
            return ExitCode::FAILURE;
        }
    };

    match cli.command {
        Some(Command::Whoami)   => commands::whoami::run(&cfg).await,
        Some(Command::Epoch)    => commands::epoch::run(&cfg).await,
        Some(Command::Download) => commands::download::run(&cfg).await,
        Some(Command::Train)    => commands::train::run(&cfg).await,
        Some(Command::Submit)   => commands::submit::run(&cfg).await,
        Some(Command::Run)      => commands::run::run(&cfg).await,
        Some(Command::Version)  => unreachable!(),
        Some(Command::Init)     => unreachable!(),
        None                    => {
            use std::sync::Arc;
            menu::run_interactive(Arc::new((*cfg).clone())).await
        }
    }
}
