use std::process::ExitCode;

use fl_client_daemon::config::Configuration;
use prettytable::{row, Table};

use crate::{coordinator::CoordinatorClient, output};

pub async fn run(cfg: &Configuration) -> ExitCode {
    let client = match CoordinatorClient::new(cfg) {
        Ok(c) => c,
        Err(e) => { output::error(&format!("Failed to build coordinator client: {e}")); return ExitCode::FAILURE; }
    };

    match client.get_active_epoch().await {
        Ok(Some(ep)) => {
            let mut t = Table::new();
            t.add_row(row!["Epoch",         ep.epoch_number]);
            t.add_row(row!["Model ID",      ep.model_id]);
            t.add_row(row!["Model Version", ep.model_version]);
            t.add_row(row!["Status",        ep.status.as_deref().unwrap_or("ACTIVE")]);
            t.add_row(row!["FedProx μ",     ep.fedprox_mu.unwrap_or(0.0)]);
            t.add_row(row!["Privacy ε",     ep.privacy_epsilon.unwrap_or(1.0)]);
            t.add_row(row!["Model Hash",    &ep.model_hash[..16]]);
            t.printstd();
            ExitCode::SUCCESS
        }
        Ok(None) => {
            let model = cfg.models.first().map(|m| m.model_id.as_str()).unwrap_or("unknown");
            println!("No active epoch for model {model}");
            ExitCode::SUCCESS
        }
        Err(e) => { output::error(&format!("Coordinator error: {e}")); ExitCode::FAILURE }
    }
}
