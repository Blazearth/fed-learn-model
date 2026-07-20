use std::process::ExitCode;

use fl_client_daemon::config::Configuration;
use ring::digest::{digest, SHA256};

use crate::{coordinator::CoordinatorClient, output, progress};

pub async fn run(cfg: &Configuration) -> ExitCode {
    let client = match CoordinatorClient::new(cfg) {
        Ok(c) => c,
        Err(e) => { output::error(&format!("Failed to build coordinator client: {e}")); return ExitCode::FAILURE; }
    };

    // 1. Get active epoch
    let ep = match client.get_active_epoch().await {
        Ok(Some(e)) => e,
        Ok(None)    => { output::error("No active epoch — nothing to download."); return ExitCode::FAILURE; }
        Err(e)      => { output::error(&format!("Coordinator error: {e}")); return ExitCode::FAILURE; }
    };

    // 2. Get pre-signed download URL
    let url = match client.get_model_download_url(&ep.model_version).await {
        Ok(u) => u,
        Err(e) => { output::error(&format!("Failed to get download URL: {e}")); return ExitCode::FAILURE; }
    };

    // 3. Download with progress bar (size unknown from pre-signed URL — use spinner)
    let pb = progress::download_bar(0); // ponytail: S3 pre-signed URLs don't always have Content-Length; 0 = spinner mode
    let bytes = match client.download_bytes(&url, &pb).await {
        Ok(b) => b,
        Err(e) => { output::error(&format!("Download failed: {e}")); return ExitCode::FAILURE; }
    };

    // 4. Verify hash
    let hash_bytes = digest(&SHA256, &bytes);
    let hash_hex: String = hash_bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if hash_hex != ep.model_hash {
        output::error(&format!("Hash mismatch — expected {}, got {hash_hex}", ep.model_hash));
        return ExitCode::FAILURE;
    }

    // 5. Write model + epoch metadata
    let model_dir = &cfg.storage.model_dir;
    if let Err(e) = std::fs::create_dir_all(model_dir) {
        output::error(&format!("Cannot create model_dir: {e}")); return ExitCode::FAILURE;
    }
    let model_path = model_dir.join(format!("model_{}.npy", ep.model_version));
    if let Err(e) = std::fs::write(&model_path, &bytes) {
        output::error(&format!("Failed to write model: {e}")); return ExitCode::FAILURE;
    }
    let epoch_json = serde_json::to_string_pretty(&ep).unwrap();
    if let Err(e) = std::fs::write(model_dir.join("current_epoch.json"), epoch_json) {
        output::error(&format!("Failed to write epoch metadata: {e}")); return ExitCode::FAILURE;
    }

    output::success(&format!("✓ Model {} downloaded and verified ({} bytes)", ep.model_version, bytes.len()));
    ExitCode::SUCCESS
}
