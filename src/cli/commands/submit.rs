use std::process::ExitCode;

use fl_client_daemon::config::Configuration;
use serde_json::Value;

use crate::{coordinator::CoordinatorClient, output, progress, state::SubmissionState};

pub async fn run(cfg: &Configuration) -> ExitCode {
    let working_dir = &cfg.storage.working_dir;

    // 1. Load update metadata
    let meta: Value = match std::fs::read_to_string(working_dir.join("update_meta.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(v) => v,
        None => {
            output::error("No update metadata found. Run 'fl-client train' first.");
            return ExitCode::FAILURE;
        }
    };
    let epoch_number = match meta["epoch_number"].as_u64() {
        Some(n) => n,
        None => { output::error("Malformed update_meta.json — missing epoch_number."); return ExitCode::FAILURE; }
    };
    let sha256 = match meta["sha256"].as_str() {
        Some(s) => s.to_string(),
        None => { output::error("Malformed update_meta.json — missing sha256."); return ExitCode::FAILURE; }
    };

    // 2. Load update binary
    let update_bytes = match std::fs::read(working_dir.join("update.bin")) {
        Ok(b) => b,
        Err(_) => {
            output::error("update.bin not found. Run 'fl-client train' first.");
            return ExitCode::FAILURE;
        }
    };

    // 3. Build coordinator client
    let client = match CoordinatorClient::new(cfg) {
        Ok(c) => c,
        Err(e) => { output::error(&format!("Coordinator client error: {e}")); return ExitCode::FAILURE; }
    };

    // 4. Get upload URL
    let upload_url = match client.get_update_upload_url(epoch_number).await {
        Ok(u) => u,
        Err(e) => { output::error(&format!("Failed to get upload URL: {e}")); return ExitCode::FAILURE; }
    };

    // 5. Upload — do NOT touch SubmissionState on failure
    let pb = progress::upload_bar(update_bytes.len() as u64);
    if let Err(e) = client.upload_bytes(&upload_url, update_bytes, &pb).await {
        output::error(&format!("Upload failed: {e}"));
        return ExitCode::FAILURE;
    }

    // 6. Notify coordinator — do NOT touch SubmissionState on failure
    if let Err(e) = client.submit_complete(epoch_number, &sha256).await {
        output::error(&format!("Coordinator notification failed (upload succeeded but submission not recorded): {e}"));
        return ExitCode::FAILURE;
    }

    // 7. Only now persist state
    let mut state = SubmissionState::load(working_dir);
    if let Err(e) = state.save(epoch_number, working_dir) {
        output::warn(&format!("Submission recorded by coordinator but local state write failed: {e}"));
    }

    output::success(&format!("✓ Submission recorded — epoch #{epoch_number}"));
    ExitCode::SUCCESS
}
