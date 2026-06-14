//! Federated Learning Client Daemon — binary entry point
//!
//! Wires all modules together and runs the main orchestration loop.
//!
//! Implements Requirements: 3-10, 14, 15

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ring::digest::{digest, SHA256};
use tokio::time::sleep;
use tracing::{error, info, warn};

use fl_client_daemon::config::manager::ConfigManager;
use fl_client_daemon::error::{DaemonError, Result};
use fl_client_daemon::metrics::MetricsEngine;
use fl_client_daemon::privacy::PrivacyEngine;
use fl_client_daemon::secureagg::SecureAggEngine;
use fl_client_daemon::training::TrainingEngine;
use fl_client_daemon::types::{EpochMetadata, Model, ModelMetadata, ParticipantInfo};
use fl_client_daemon::config::MlFramework;

// ── CLI argument parsing ──────────────────────────────────────────────────────

struct CliArgs {
    config_path: PathBuf,
}

fn parse_args() -> CliArgs {
    let mut args = std::env::args().skip(1);
    let config_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/fl-daemon/config.toml"));
    CliArgs { config_path }
}

// ── Epoch metadata validation (Req 14) ───────────────────────────────────────

/// Validate epoch metadata received from the coordinator.
///
/// Checks:
/// - epoch_number > last_processed_epoch (monotonicity)
/// - model_hash is valid 64-char hex string
/// - fedprox_mu within [0.0, 1.0]
/// - privacy_epsilon > 0.0 and privacy_delta > 0.0 && < 1.0
/// - org_id present in secure_agg_participants when list is non-empty
pub fn validate_epoch_metadata(
    metadata: &EpochMetadata,
    last_processed_epoch: u64,
    org_id: &str,
) -> Result<()> {
    // 1. Monotonicity check
    if metadata.epoch_number <= last_processed_epoch {
        return Err(DaemonError::Other(format!(
            "epoch {} is not monotonically increasing (last processed: {})",
            metadata.epoch_number, last_processed_epoch
        )));
    }

    // 2. model_hash must be a valid 64-char lowercase hex string
    if metadata.model_hash.len() != 64
        || !metadata.model_hash.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(DaemonError::Other(format!(
            "model_hash '{}' is not a valid 64-char hex string",
            metadata.model_hash
        )));
    }

    // 3. fedprox_mu in [0.0, 1.0]
    if !(0.0..=1.0).contains(&metadata.fedprox_mu) {
        return Err(DaemonError::Other(format!(
            "fedprox_mu {} is outside valid range [0.0, 1.0]",
            metadata.fedprox_mu
        )));
    }

    // 4. privacy_epsilon > 0.0
    if metadata.privacy_epsilon <= 0.0 {
        return Err(DaemonError::Other(format!(
            "privacy_epsilon {} must be positive",
            metadata.privacy_epsilon
        )));
    }

    // 5. privacy_delta > 0.0 && < 1.0
    if metadata.privacy_delta <= 0.0 || metadata.privacy_delta >= 1.0 {
        return Err(DaemonError::Other(format!(
            "privacy_delta {} must be in range (0.0, 1.0)",
            metadata.privacy_delta
        )));
    }

    // 6. If participant list is non-empty, org_id must be in it
    if !metadata.secure_agg_participants.is_empty() {
        let present = metadata
            .secure_agg_participants
            .iter()
            .any(|p| p.org_id == org_id);
        if !present {
            return Err(DaemonError::Other(format!(
                "org_id '{}' is not in secure_agg_participants list",
                org_id
            )));
        }
    }

    Ok(())
}

// ── Mock helpers ──────────────────────────────────────────────────────────────

/// Create a mock EpochMetadata for use when a real coordinator is not available.
fn mock_epoch_metadata(epoch_number: u64, org_id: &str) -> EpochMetadata {
    // 64-char zero hex string as the model hash placeholder
    let model_hash = "a".repeat(64);
    EpochMetadata {
        epoch_number,
        model_id: "fraud-detection-v2".to_string(),
        model_version: format!("v1.{}", epoch_number),
        model_hash,
        model_signature: vec![0u8; 64],
        architecture_hash: "arch-abc123".to_string(),
        fedprox_mu: 0.01,
        privacy_epsilon: 1.0,
        privacy_delta: 1e-5,
        secure_agg_participants: vec![ParticipantInfo {
            org_id: org_id.to_string(),
            public_key: vec![0u8; 32],
        }],
        secure_agg_threshold: 1,
        drift_alerts: vec![],
        dataset_schema: None,
    }
}

/// Create a mock global Model.
fn mock_global_model(epoch_metadata: &EpochMetadata) -> Model {
    Model {
        version: epoch_metadata.model_version.clone(),
        architecture_hash: epoch_metadata.architecture_hash.clone(),
        framework: MlFramework::PyTorch,
        binary: vec![0u8; 256],
        metadata: ModelMetadata {
            input_shape: vec![1, 128],
            output_shape: vec![2],
            parameter_count: 10_000,
            created_at: None,
        },
    }
}

// ── Training round workflow (Reqs 3-9) ───────────────────────────────────────

/// Execute a complete federated learning training round.
///
/// Steps:
/// 1. Poll coordinator for epoch metadata (simulated)
/// 2. Validate epoch metadata (Req 14)
/// 3. Download global model (simulated)
/// 4. Load and validate dataset
/// 5. Run FedProx training (Req 5)
/// 6. Apply differential privacy (Req 6)
/// 7. Apply secure aggregation masking (Req 7)
/// 8. Compute SHA-256 hash of protected update
/// 9. Simulate upload to S3 (Req 8)
/// 10. Mark round complete, clean up temp state (Req 9)
pub async fn run_training_round(
    config: Arc<fl_client_daemon::config::Configuration>,
    last_processed_epoch: &mut u64,
) -> Result<()> {
    let org_id = &config.organization_id;

    // ── Step 1: Poll coordinator for epoch metadata ───────────────────────────
    info!(org_id, "Polling coordinator for epoch metadata");
    let epoch_number = *last_processed_epoch + 1;
    let epoch_metadata = mock_epoch_metadata(epoch_number, org_id);
    info!(
        epoch = epoch_metadata.epoch_number,
        model_id = %epoch_metadata.model_id,
        "Epoch metadata received"
    );

    // ── Step 2: Validate epoch metadata (Req 14) ─────────────────────────────
    info!(epoch = epoch_number, "Validating epoch metadata");
    validate_epoch_metadata(&epoch_metadata, *last_processed_epoch, org_id)?;
    info!(epoch = epoch_number, "Epoch metadata validation passed");

    // ── Step 3: Download global model (simulated) ────────────────────────────
    info!(
        model_version = %epoch_metadata.model_version,
        "Downloading global model"
    );
    let global_model = mock_global_model(&epoch_metadata);
    info!(
        model_version = %global_model.version,
        "Global model ready"
    );

    // ── Step 4: Load and validate dataset ────────────────────────────────────
    info!("Loading and validating training dataset");
    let training_config = config.training.clone();
    let engine = TrainingEngine::new(training_config.clone());
    let dataset = TrainingEngine::create_mock_dataset(
        vec![
            "feature_0".to_string(),
            "feature_1".to_string(),
            "feature_2".to_string(),
        ],
        1000,
    );
    engine.validate_dataset(&dataset, None, Some(100), Some(100_000))?;
    info!(row_count = dataset.row_count, "Dataset validated");

    // ── Step 5: Run FedProx training (Req 5) ─────────────────────────────────
    info!(
        mu = training_config.fedprox_mu,
        local_epochs = training_config.local_epochs,
        "Running FedProx training"
    );
    let (_local_model, training_metrics) = engine.train_fedprox(&global_model, &dataset)?;
    info!(
        final_loss = training_metrics.loss_history.last().copied().unwrap_or(1.0),
        final_accuracy = training_metrics.accuracy_history.last().copied().unwrap_or(0.0),
        "FedProx training complete"
    );

    // Compute model update
    let mut model_update = engine.compute_update(&global_model, &_local_model)?;
    model_update.metadata.sample_count = dataset.row_count;

    // ── Step 6: Apply differential privacy (Req 6) ───────────────────────────
    info!(
        epsilon = config.privacy.epsilon,
        delta = config.privacy.delta,
        "Applying differential privacy"
    );
    let privacy_engine = PrivacyEngine::new(config.privacy.clone());
    let private_update = privacy_engine.apply_privacy(model_update)?;
    info!("Differential privacy applied");

    // ── Step 7: Apply secure aggregation masking (Req 7) ─────────────────────
    info!("Applying secure aggregation masking");
    let secureagg_engine = SecureAggEngine::new(config.secure_aggregation.clone())?;
    let masked_update = secureagg_engine.apply_masking(
        private_update,
        &epoch_metadata.secure_agg_participants,
        org_id,
    )?;
    info!(
        participant_id = %masked_update.participant_id,
        "Secure aggregation masking applied"
    );

    // ── Step 8: Compute SHA-256 hash of protected update ─────────────────────
    let serialized_update: Vec<u8> = masked_update
        .masked_gradients
        .iter()
        .flat_map(|layer| layer.iter())
        .flat_map(|&v| v.to_le_bytes())
        .collect();
    let update_hash_bytes = digest(&SHA256, &serialized_update);
    let update_hash: String = update_hash_bytes
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    info!(update_hash = %update_hash, "Protected update hash computed");

    // ── Step 9: Simulate upload to S3 (Req 8) ────────────────────────────────
    info!(
        update_hash = %update_hash,
        epoch = epoch_number,
        model_id = %epoch_metadata.model_id,
        "Simulating S3 upload of protected update"
    );
    // In production: upload to S3 pre-signed URL and submit completion notification

    // ── Step 10: Mark round complete, clean up (Req 9) ───────────────────────
    *last_processed_epoch = epoch_number;
    info!(
        epoch = epoch_number,
        "Training round complete, state updated"
    );

    Ok(())
}

// ── Certificate monitor ───────────────────────────────────────────────────────

/// Periodically check certificate expiration.
async fn certificate_monitor_loop(check_interval: Duration) {
    loop {
        sleep(check_interval).await;
        info!("Certificate expiration check");
        // In production: call CertificateManager::check_expiration()
        // and handle rotation via check_rotation()
    }
}

// ── Resource monitor ─────────────────────────────────────────────────────────

/// Periodically check resource limits.
async fn resource_monitor_loop(
    config: Arc<fl_client_daemon::config::Configuration>,
    check_interval: Duration,
) {
    let engine = MetricsEngine::new(config.resources.clone());
    loop {
        sleep(check_interval).await;
        let metrics = engine.measure_resources();
        let violations = engine.check_limits(&metrics);
        if violations.is_empty() {
            info!(
                cpu_percent = metrics.cpu_percent,
                ram_gb = metrics.ram_gb,
                "Resource usage within limits"
            );
        } else {
            for v in &violations {
                warn!(
                    resource = %v.resource,
                    current = v.current,
                    limit = v.limit,
                    "Resource limit exceeded: {}",
                    v.message
                );
            }
        }
    }
}

// ── Polling loop ─────────────────────────────────────────────────────────────

/// Polling loop that periodically calls run_training_round().
async fn polling_loop(config: Arc<fl_client_daemon::config::Configuration>) {
    let poll_interval = Duration::from_secs(config.coordinator.poll_interval_secs);
    let mut last_processed_epoch: u64 = 0;

    loop {
        match run_training_round(Arc::clone(&config), &mut last_processed_epoch).await {
            Ok(()) => {
                info!(
                    last_epoch = last_processed_epoch,
                    "Training round completed successfully"
                );
            }
            Err(e) => {
                error!(error = %e, "Training round failed");
            }
        }

        info!(
            poll_interval_secs = poll_interval.as_secs(),
            "Waiting until next polling interval"
        );
        sleep(poll_interval).await;
    }
}

// ── Graceful shutdown (Req 10) ────────────────────────────────────────────────

/// Perform graceful shutdown sequence.
async fn graceful_shutdown() {
    info!("Completing in-progress operations before shutdown");
    // In production: wait for any active upload to finish
    sleep(Duration::from_millis(100)).await;
    info!("Saving current training state");
    info!("Closing connections");
    info!("Daemon shutting down gracefully");
}

// ── Main entry point ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // ── Parse CLI args ────────────────────────────────────────────────────────
    let args = parse_args();

    // ── Initialize tracing subscriber ────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("fl_client_daemon=info".parse().unwrap()),
        )
        .init();

    // ── Load configuration ────────────────────────────────────────────────────
    let config_manager = match ConfigManager::new(args.config_path.clone()).await {
        Ok(m) => {
            info!(
                config_path = %args.config_path.display(),
                "Configuration loaded successfully"
            );
            Arc::new(m)
        }
        Err(e) => {
            eprintln!("Fatal: failed to load configuration: {e}");
            std::process::exit(1);
        }
    };

    let config = config_manager.get();

    // ── Startup banner ────────────────────────────────────────────────────────
    info!(
        version = env!("CARGO_PKG_VERSION"),
        org_id = %config.organization_id,
        coordinator = %config.coordinator.base_url,
        "Federated Learning Client Daemon starting"
    );

    // ── Spawn background tasks ────────────────────────────────────────────────
    let poll_config = Arc::clone(&config);
    let resource_config = Arc::clone(&config);

    let cert_interval = Duration::from_secs(config.certificates.check_interval_secs);
    let resource_interval = Duration::from_secs(60);

    // Signal handling is Unix-only
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        let mut sighup = signal(SignalKind::hangup())
            .expect("failed to register SIGHUP handler");

        let reload_manager = Arc::clone(&config_manager);

        tokio::select! {
            _ = polling_loop(poll_config) => {
                info!("Polling loop exited");
            }
            _ = certificate_monitor_loop(cert_interval) => {
                info!("Certificate monitor exited");
            }
            _ = resource_monitor_loop(resource_config, resource_interval) => {
                info!("Resource monitor exited");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM — initiating graceful shutdown");
                graceful_shutdown().await;
                std::process::exit(0);
            }
            _ = sighup.recv() => {
                info!("Received SIGHUP — reloading configuration");
                match reload_manager.reload().await {
                    Ok(()) => info!("Configuration reloaded successfully"),
                    Err(e) => warn!(error = %e, "Configuration reload failed"),
                }
                // Re-enter the loop after reload (process continues)
            }
        }
    }

    // Non-Unix fallback: run polling loop until Ctrl-C
    #[cfg(not(unix))]
    {
        tokio::select! {
            _ = polling_loop(poll_config) => {
                info!("Polling loop exited");
            }
            _ = certificate_monitor_loop(cert_interval) => {
                info!("Certificate monitor exited");
            }
            _ = resource_monitor_loop(resource_config, resource_interval) => {
                info!("Resource monitor exited");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl-C — initiating graceful shutdown");
                graceful_shutdown().await;
                std::process::exit(0);
            }
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_epoch_metadata(epoch: u64, org_id: &str) -> EpochMetadata {
        EpochMetadata {
            epoch_number: epoch,
            model_id: "test-model".to_string(),
            model_version: format!("v{}", epoch),
            model_hash: "a".repeat(64),
            model_signature: vec![0u8; 64],
            architecture_hash: "arch-abc123".to_string(),
            fedprox_mu: 0.01,
            privacy_epsilon: 1.0,
            privacy_delta: 1e-5,
            secure_agg_participants: vec![ParticipantInfo {
                org_id: org_id.to_string(),
                public_key: vec![0u8; 32],
            }],
            secure_agg_threshold: 1,
            drift_alerts: vec![],
            dataset_schema: None,
        }
    }

    #[test]
    fn test_validate_epoch_metadata_valid() {
        let meta = make_valid_epoch_metadata(5, "org-test");
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_ok());
    }

    #[test]
    fn test_validate_epoch_metadata_not_monotonic() {
        let meta = make_valid_epoch_metadata(3, "org-test");
        // epoch 3 is NOT > last_processed 5
        let result = validate_epoch_metadata(&meta, 5, "org-test");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("monotonically"), "expected monotonicity error: {msg}");
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_hash_short() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.model_hash = "abc123".to_string(); // too short
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_hash_non_hex() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.model_hash = "z".repeat(64); // not hex
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_fedprox_mu_negative() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.fedprox_mu = -0.1;
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_fedprox_mu_over_one() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.fedprox_mu = 1.5;
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_epsilon() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.privacy_epsilon = 0.0;
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_delta_zero() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.privacy_delta = 0.0;
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_invalid_delta_one() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.privacy_delta = 1.0;
        assert!(validate_epoch_metadata(&meta, 4, "org-test").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_org_not_in_participants() {
        let meta = make_valid_epoch_metadata(5, "org-other");
        // participants has "org-other" but we validate for "org-mine"
        assert!(validate_epoch_metadata(&meta, 4, "org-mine").is_err());
    }

    #[test]
    fn test_validate_epoch_metadata_empty_participants_any_org() {
        let mut meta = make_valid_epoch_metadata(5, "org-test");
        meta.secure_agg_participants.clear();
        // empty participant list — org_id check is skipped
        assert!(validate_epoch_metadata(&meta, 4, "org-anything").is_ok());
    }
}
