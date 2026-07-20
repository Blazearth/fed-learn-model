use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fl-client", version, about = "Federated Learning CLI")]
pub struct Cli {
    /// Path to config file (overrides default search order)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show organization identity from config
    Whoami,
    /// Query active training epoch from coordinator
    Epoch,
    /// Download latest global model from S3
    Download,
    /// Run local FedProx training and apply privacy
    Train,
    /// Upload protected update and notify coordinator
    Submit,
    /// Run full pipeline: epoch → download → train → submit
    Run,
    /// Show CLI version
    Version,
    /// Interactive setup wizard — create config.toml
    Init,
}
