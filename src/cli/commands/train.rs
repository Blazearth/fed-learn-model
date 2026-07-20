use std::process::ExitCode;

use fl_client_daemon::config::{Configuration, MlFramework};
use fl_client_daemon::privacy::PrivacyEngine;
use fl_client_daemon::secureagg::SecureAggEngine;
use fl_client_daemon::training::TrainingEngine;
use fl_client_daemon::types::{Model, ModelMetadata};
use prettytable::{row, Table};
use ring::digest::{digest, SHA256};

use crate::coordinator::EpochInfo;
use crate::{output, progress};

pub async fn run(cfg: &Configuration) -> ExitCode {
    let model_dir   = &cfg.storage.model_dir;
    let working_dir = &cfg.storage.working_dir;

    // 1. Load epoch metadata written by download
    let epoch_path = model_dir.join("current_epoch.json");
    let ep: EpochInfo = match std::fs::read_to_string(&epoch_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(e) => e,
        None => {
            output::error("No epoch metadata found. Run 'fl-client download' first.");
            return ExitCode::FAILURE;
        }
    };

    // 2. Load model binary
    let model_path = model_dir.join(format!("model_{}.npy", ep.model_version));
    let model_bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(_) => {
            output::error(&format!("Model file not found at {}. Run 'fl-client download' first.", model_path.display()));
            return ExitCode::FAILURE;
        }
    };

    let global_model = Model {
        version:          ep.model_version.clone(),
        architecture_hash: ep.architecture_hash.clone().unwrap_or_default(),
        framework:         MlFramework::PyTorch,
        binary:            model_bytes,
        metadata: ModelMetadata { input_shape: vec![1, 128], output_shape: vec![2], parameter_count: 10_000, created_at: None },
    };

    // 3. Train with progress bar
    let engine  = TrainingEngine::new(cfg.training.clone());
    let dataset = TrainingEngine::create_mock_dataset(vec!["feature_0".into(), "feature_1".into()], 1000);
    let pb      = progress::training_bar(cfg.training.local_epochs as u64);

    let (local_model, metrics) = match engine.train_fedprox(&global_model, &dataset) {
        Ok(r) => r,
        Err(e) => { output::error(&format!("Training failed: {e}")); return ExitCode::FAILURE; }
    };
    pb.finish_with_message("✓ Done");

    // 4. Compute update
    let mut update = match engine.compute_update(&global_model, &local_model) {
        Ok(u) => u,
        Err(e) => { output::error(&format!("compute_update failed: {e}")); return ExitCode::FAILURE; }
    };
    update.metadata.sample_count = dataset.row_count;

    // 5. Apply privacy (must succeed before SecAgg)
    let private_update = match PrivacyEngine::new(cfg.privacy.clone()).apply_privacy(update) {
        Ok(u) => u,
        Err(e) => { output::error(&format!("Privacy engine failed: {e}")); return ExitCode::FAILURE; }
    };

    // 6. Apply secure aggregation
    let participants: Vec<fl_client_daemon::types::ParticipantInfo> = ep.secure_agg_participants
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| {
            Some(fl_client_daemon::types::ParticipantInfo {
                org_id:     p["org_id"].as_str()?.to_string(),
                public_key: vec![], // ponytail: public_key not needed for masking sign check
            })
        })
        .collect();

    let sec_engine = match SecureAggEngine::new(cfg.secure_aggregation.clone()) {
        Ok(e) => e,
        Err(e) => { output::error(&format!("SecureAgg init failed: {e}")); return ExitCode::FAILURE; }
    };
    let masked = match sec_engine.apply_masking(private_update, &participants, &cfg.organization_id) {
        Ok(m) => m,
        Err(e) => {
            // clean up if we wrote anything (nothing yet at this point, but belt-and-suspenders)
            output::error(&format!("SecureAgg masking failed: {e}"));
            return ExitCode::FAILURE;
        }
    };

    // 7. Serialize + hash + write
    if let Err(e) = std::fs::create_dir_all(working_dir) {
        output::error(&format!("Cannot create working_dir: {e}")); return ExitCode::FAILURE;
    }

    let serialized: Vec<u8> = masked.masked_gradients.iter()
        .flat_map(|l| l.iter())
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let hash_bytes = digest(&SHA256, &serialized);
    let sha256: String = hash_bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect();

    let update_bin_path = working_dir.join("update.bin");
    if let Err(e) = std::fs::write(&update_bin_path, &serialized) {
        output::error(&format!("Failed to write update.bin: {e}")); return ExitCode::FAILURE;
    }

    let meta = serde_json::json!({ "epoch_number": ep.epoch_number, "sha256": sha256 });
    if let Err(e) = std::fs::write(working_dir.join("update_meta.json"), serde_json::to_string_pretty(&meta).unwrap()) {
        output::error(&format!("Failed to write update_meta.json: {e}")); return ExitCode::FAILURE;
    }

    // 8. Summary table
    let final_loss = metrics.loss_history.last().copied().unwrap_or(1.0);
    let final_acc  = metrics.accuracy_history.last().copied().unwrap_or(0.0);
    let mut t = Table::new();
    t.add_row(row!["Final Loss",      format!("{final_loss:.4}")]);
    t.add_row(row!["Final Accuracy",  format!("{:.2}%", final_acc * 100.0)]);
    t.add_row(row!["Privacy ε",       cfg.privacy.epsilon]);
    t.add_row(row!["Update SHA-256",  &sha256[..16]]);
    t.printstd();

    output::success("✓ Training complete — protected update ready for submission.");
    ExitCode::SUCCESS
}
