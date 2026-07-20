use std::process::ExitCode;

use fl_client_daemon::config::Configuration;

use crate::{coordinator::CoordinatorClient, output, state::SubmissionState};
use super::{download, submit, train};

pub async fn run(cfg: &Configuration) -> ExitCode {
    // 1. Get active epoch (needed for idempotency check before any file I/O)
    let client = match CoordinatorClient::new(cfg) {
        Ok(c) => c,
        Err(e) => { output::error(&format!("Coordinator client error: {e}")); return ExitCode::FAILURE; }
    };
    let ep = match client.get_active_epoch().await {
        Ok(Some(e)) => e,
        Ok(None)    => { output::error("No active epoch. Nothing to run."); return ExitCode::FAILURE; }
        Err(e)      => { output::error(&format!("Coordinator error: {e}")); return ExitCode::FAILURE; }
    };

    // 2. Idempotency check
    let state = SubmissionState::load(&cfg.storage.working_dir);
    if state.is_submitted(ep.epoch_number) {
        println!("Epoch {} already submitted — nothing to do.", ep.epoch_number);
        return ExitCode::SUCCESS;
    }

    // 3. Pipeline — abort on first failure
    for (name, result) in [
        ("download", download::run(cfg).await),
        ("train",    train::run(cfg).await),
        ("submit",   submit::run(cfg).await),
    ] {
        if result != ExitCode::SUCCESS {
            output::error(&format!("Pipeline failed at step: {name}"));
            return ExitCode::FAILURE;
        }
    }

    output::success("✓ Full pipeline complete.");
    ExitCode::SUCCESS
}
