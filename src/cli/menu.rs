use std::process::ExitCode;
use std::sync::Arc;

use dialoguer::Select;
use fl_client_daemon::config::Configuration;

use crate::{commands, output, state::SubmissionState};

const ITEMS: &[&str] = &[
    "1. View Active Epoch",
    "2. Download Model",
    "3. Train Model",
    "4. Submit Update",
    "5. Run Full Pipeline",
    "6. View Status",
    "0. Exit",
];

pub async fn run_interactive(cfg: Arc<Configuration>) -> ExitCode {
    print_header(&cfg);

    loop {
        let choice = Select::new()
            .items(ITEMS)
            .default(0)
            .interact();

        match choice {
            Ok(0) => { commands::epoch::run(&cfg).await; }
            Ok(1) => { commands::download::run(&cfg).await; }
            Ok(2) => { commands::train::run(&cfg).await; }
            Ok(3) => { commands::submit::run(&cfg).await; }
            Ok(4) => { commands::run::run(&cfg).await; }
            Ok(5) => {
                let state = SubmissionState::load(&cfg.storage.working_dir);
                commands::status::run(&cfg, &state);
            }
            Ok(6) | Err(_) => break,
            _ => {}
        }
        print_header(&cfg);
    }

    ExitCode::SUCCESS
}

fn print_header(cfg: &Configuration) {
    println!("\n{}", "═".repeat(42));
    println!(" Federated Learning Client");
    println!(" {} · {}", cfg.organization_id, cfg.coordinator.base_url);
    println!("{}\n", "═".repeat(42));
}

pub fn no_config_error() {
    output::error("No config found. Run 'fl-client init' to create one.");
}
