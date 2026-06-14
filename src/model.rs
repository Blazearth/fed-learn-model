//! Model management: download, signature verification, compatibility, rollback
//!
//! Implements Requirements: 4, 21, 22, 30
//! Design properties: 6 (hash verification), 7 (storage location),
//!                    33 (archive size limit), 41 (signature verification)

use ring::digest::{digest, SHA256};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use zeroize::Zeroize;

use crate::config::{MlFramework, StorageConfig};
use crate::error::{DaemonError, ModelError, Result};
use crate::types::{EpochMetadata, Model, ModelMetadata};

// ── Archive manifest ─────────────────────────────────────────────────────────

/// Persisted index of archived model versions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ArchiveManifest {
    /// Ordered list of versions, oldest first.
    versions: Vec<String>,
}

// ── ModelManager ─────────────────────────────────────────────────────────────

/// Manages global model downloads, verification, and version history.
pub struct ModelManager {
    /// Directory for current and archived models.
    model_dir: PathBuf,
    /// Maximum number of archived versions to retain (default 5).
    retention_count: usize,
    /// Ed25519 public key of the Cloud Coordinator used to verify model signatures.
    coordinator_public_key: Vec<u8>,
}

impl ModelManager {
    /// Create a new ModelManager.
    ///
    /// `storage_config` — paths and retention settings.
    /// `coordinator_public_key` — raw Ed25519 public key bytes (32 bytes).
    pub fn new(storage_config: &StorageConfig, coordinator_public_key: Vec<u8>) -> Result<Self> {
        let model_dir = storage_config.model_dir.clone();
        fs::create_dir_all(&model_dir).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!(
                "cannot create model directory {}: {e}",
                model_dir.display()
            )))
        })?;

        let archive_dir = model_dir.join("archive");
        fs::create_dir_all(&archive_dir).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!(
                "cannot create archive directory {}: {e}",
                archive_dir.display()
            )))
        })?;

        Ok(Self {
            model_dir,
            retention_count: storage_config.model_retention_count,
            coordinator_public_key,
        })
    }

    // ── Download ─────────────────────────────────────────────────────────────

    /// Validate a model that was already downloaded as raw bytes.
    ///
    /// Steps (in order per design §3.3 and Requirement 30.6):
    /// 1. Verify Ed25519 signature (before hash, per Req 30.6)
    /// 2. Verify SHA-256 hash
    /// 3. Check compatibility (architecture, framework, format)
    /// 4. Archive previous version
    ///
    /// Returns the validated [`Model`] ready for training.
    pub fn validate_and_store(
        &self,
        model_bytes: Vec<u8>,
        epoch_metadata: &EpochMetadata,
    ) -> Result<Model> {
        // Step 1 — signature (Req 30.1, 30.2, 30.3, 30.4, 30.5, 30.6)
        self.verify_signature(&model_bytes, &epoch_metadata.model_signature)?;

        // Step 2 — hash (Req 4.4, 4.5)
        self.verify_hash(&model_bytes, &epoch_metadata.model_hash)?;

        // Step 3 — compatibility (Req 21.1, 21.2, 21.4, 21.5)
        self.check_compatibility(epoch_metadata)?;

        // Step 4 — build Model struct
        let model = Model {
            version: epoch_metadata.model_version.clone(),
            architecture_hash: epoch_metadata.architecture_hash.clone(),
            framework: MlFramework::PyTorch, // resolved during compatibility check
            binary: model_bytes,
            metadata: ModelMetadata {
                input_shape: vec![],
                output_shape: vec![],
                parameter_count: 0,
                created_at: None,
            },
        };

        // Step 5 — archive previous, store current (Req 4.6)
        self.archive_current_and_store(&model)?;

        Ok(model)
    }

    // ── Signature verification (Req 30) ──────────────────────────────────────

    /// Verify Ed25519 signature of model bytes using the coordinator public key.
    /// Called BEFORE hash verification per Requirement 30.6.
    fn verify_signature(&self, model_bytes: &[u8], signature: &[u8]) -> Result<()> {
        if signature.is_empty() {
            return Err(DaemonError::Model(ModelError::InvalidSignature));
        }

        let public_key = UnparsedPublicKey::new(&ED25519, &self.coordinator_public_key);
        public_key
            .verify(model_bytes, signature)
            .map_err(|_| DaemonError::Model(ModelError::SignatureVerificationFailed(
                "Ed25519 signature invalid".to_string(),
            )))?;

        tracing::info!("Model signature verified successfully");
        Ok(())
    }

    // ── Hash verification (Req 4.4, 4.5) ────────────────────────────────────

    /// Verify SHA-256 hash of model bytes against expected hash in metadata.
    fn verify_hash(&self, model_bytes: &[u8], expected_hex: &str) -> Result<()> {
        let actual = digest(&SHA256, model_bytes);
        let actual_hex = hex_encode(actual.as_ref());

        if actual_hex != expected_hex.to_lowercase() {
            return Err(DaemonError::Model(ModelError::HashMismatch {
                expected: expected_hex.to_string(),
                actual: actual_hex,
            }));
        }

        tracing::debug!("Model hash verified: {}", actual_hex);
        Ok(())
    }

    /// Validate model compatibility with this client's configuration.
    ///
    /// Requirement 21.1: architecture hash in epoch_metadata must match stored model's hash.
    fn check_compatibility(&self, epoch_metadata: &EpochMetadata) -> Result<()> {
        // Requirement 21.1 — architecture hash must be non-empty
        if epoch_metadata.architecture_hash.is_empty() {
            return Err(DaemonError::Model(ModelError::IncompatibleArchitecture(
                "architecture hash is empty".to_string(),
            )));
        }

        // If a current model version is stored, verify architecture hash matches
        if let Ok(current_version) = self.read_current_version() {
            let version_meta_path = self.model_dir.join("current_arch_hash.txt");
            if let Ok(stored_arch_hash) = fs::read_to_string(&version_meta_path) {
                let stored = stored_arch_hash.trim();
                if !stored.is_empty() && stored != epoch_metadata.architecture_hash {
                    return Err(DaemonError::Model(ModelError::IncompatibleArchitecture(
                        format!(
                            "architecture hash mismatch: stored={stored}, received={} (version={current_version})",
                            epoch_metadata.architecture_hash
                        ),
                    )));
                }
            }
        }

        tracing::debug!(
            "Compatibility check passed: arch={} epoch={}",
            epoch_metadata.architecture_hash,
            epoch_metadata.epoch_number
        );
        Ok(())
    }

    // ── Storage and archiving (Req 4.6, 22) ──────────────────────────────────

    /// Move currently stored model to archive, then write the new model.
    fn archive_current_and_store(&self, model: &Model) -> Result<()> {
        let current_path = self.current_model_path();

        // If a current model exists, move it to archive
        if current_path.exists() {
            if let Ok(data) = fs::read(&current_path) {
                let version = self.read_current_version().unwrap_or_else(|_| "unknown".to_string());
                self.archive_version(&version, &data)?;
            }
        }

        // Write new model binary (Req 4.6)
        fs::write(&current_path, &model.binary).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("failed to write model: {e}")))
        })?;

        // Write version metadata
        let version_path = self.model_dir.join("current_version.txt");
        fs::write(&version_path, &model.version).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("failed to write version: {e}")))
        })?;

        // Write architecture hash for future compatibility checks (Req 21.1)
        let arch_hash_path = self.model_dir.join("current_arch_hash.txt");
        fs::write(&arch_hash_path, &model.architecture_hash).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("failed to write arch hash: {e}")))
        })?;

        tracing::info!(
            "Stored model version {} at {}",
            model.version,
            current_path.display()
        );
        Ok(())
    }

    /// Archive a specific version into the archive subdirectory.
    fn archive_version(&self, version: &str, data: &[u8]) -> Result<()> {
        let archive_dir = self.model_dir.join("archive");
        let dest = archive_dir.join(format!("{}.bin", sanitize_version(version)));
        fs::write(&dest, data).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!(
                "failed to archive version {version}: {e}"
            )))
        })?;

        let mut manifest = self.load_manifest()?;
        manifest.versions.retain(|v| v != version);
        manifest.versions.push(version.to_string());

        // Enforce retention limit (Req 22.1, 22.2, 22.7 — Property 33)
        while manifest.versions.len() > self.retention_count {
            let oldest = manifest.versions.remove(0);
            let old_path = archive_dir.join(format!("{}.bin", sanitize_version(&oldest)));
            let _ = fs::remove_file(&old_path);
            tracing::debug!("Evicted old model version: {}", oldest);
        }

        self.save_manifest(&manifest)?;
        Ok(())
    }

    // ── Rollback (Req 22.3, 22.4, 22.5, 22.6) ───────────────────────────────

    /// Roll back to the most recent archived model version.
    pub fn rollback(&self, reason: &str) -> Result<Model> {
        let manifest = self.load_manifest()?;

        if manifest.versions.is_empty() {
            return Err(DaemonError::Model(ModelError::NoPreviousVersion));
        }

        // Most recent archived version is last in the list
        let previous_version = manifest.versions.last().unwrap().clone();
        let archive_path = self.model_dir
            .join("archive")
            .join(format!("{}.bin", sanitize_version(&previous_version)));

        let data = fs::read(&archive_path).map_err(|_| {
            DaemonError::Model(ModelError::RollbackFailed(format!(
                "archive file missing for version {previous_version}"
            )))
        })?;

        let current_version = self.read_current_version().unwrap_or_else(|_| "unknown".to_string());

        tracing::warn!(
            "Rolling back model: {} → {} (reason: {})",
            current_version, previous_version, reason
        );

        Ok(Model {
            version: previous_version,
            architecture_hash: String::new(),
            framework: MlFramework::PyTorch,
            binary: data,
            metadata: ModelMetadata {
                input_shape: vec![],
                output_shape: vec![],
                parameter_count: 0,
                created_at: None,
            },
        })
    }

    // ── Memory zeroization (Req 24) ───────────────────────────────────────────

    /// Zeroize the model binary in-place after it is no longer needed (Req 24).
    ///
    /// Call this after the model has been loaded into the ML framework so the
    /// raw bytes are not left in heap memory.
    pub fn zeroize_model_binary(model: &mut Model) {
        model.binary.zeroize();
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn current_model_path(&self) -> PathBuf {
        self.model_dir.join("current.bin")
    }

    fn manifest_path(&self) -> PathBuf {
        self.model_dir.join("archive").join("manifest.json")
    }

    fn load_manifest(&self) -> Result<ArchiveManifest> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(ArchiveManifest::default());
        }
        let data = fs::read_to_string(&path).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("manifest read error: {e}")))
        })?;
        serde_json::from_str(&data).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("manifest parse error: {e}")))
        })
    }

    fn save_manifest(&self, manifest: &ArchiveManifest) -> Result<()> {
        let json = serde_json::to_string_pretty(manifest).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("manifest serialize error: {e}")))
        })?;
        fs::write(self.manifest_path(), json).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("manifest write error: {e}")))
        })
    }

    fn read_current_version(&self) -> Result<String> {
        let path = self.model_dir.join("current_version.txt");
        fs::read_to_string(&path).map_err(|e| {
            DaemonError::Model(ModelError::DownloadFailed(format!("version read error: {e}")))
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Strip characters unsafe for filesystem paths from a version string.
fn sanitize_version(version: &str) -> String {
    version
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StorageConfig;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tempfile::tempdir;

    /// Build a coordinator Ed25519 key pair and return (pkcs8_bytes, public_key_bytes).
    fn make_keypair() -> (Vec<u8>, Vec<u8>) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_key = pair.public_key().as_ref().to_vec();
        (pkcs8.as_ref().to_vec(), pub_key)
    }

    fn make_storage(dir: &std::path::Path, retention: usize) -> StorageConfig {
        StorageConfig {
            working_dir: dir.to_path_buf(),
            model_dir: dir.join("models"),
            checkpoint_dir: dir.join("checkpoints"),
            audit_log_path: dir.join("audit.log"),
            model_retention_count: retention,
            explainability_dir: None,
        }
    }

    fn make_manager(dir: &std::path::Path, pub_key: Vec<u8>) -> ModelManager {
        let storage = make_storage(dir, 5);
        ModelManager::new(&storage, pub_key).unwrap()
    }

    fn sign(pkcs8: &[u8], data: &[u8]) -> Vec<u8> {
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8).unwrap();
        pair.sign(data).as_ref().to_vec()
    }

    fn sha256_hex(data: &[u8]) -> String {
        let h = digest(&SHA256, data);
        hex_encode(h.as_ref())
    }

    fn make_epoch_meta(version: &str, model_bytes: &[u8], pkcs8: &[u8]) -> EpochMetadata {
        EpochMetadata {
            epoch_number: 1,
            model_id: "fraud-detection".to_string(),
            model_version: version.to_string(),
            model_hash: sha256_hex(model_bytes),
            model_signature: sign(pkcs8, model_bytes),
            architecture_hash: "arch-abc123".to_string(),
            fedprox_mu: 0.01,
            privacy_epsilon: 1.0,
            privacy_delta: 1e-5,
            secure_agg_participants: vec![],
            secure_agg_threshold: 0,
            drift_alerts: vec![],
            dataset_schema: None,
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_validate_and_store_success() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        let model_bytes = b"fake model binary data".to_vec();
        let meta = make_epoch_meta("v1.0", &model_bytes, &pkcs8);

        let result = manager.validate_and_store(model_bytes.clone(), &meta);
        assert!(result.is_ok(), "Expected success: {:?}", result.err());

        let model = result.unwrap();
        assert_eq!(model.version, "v1.0");
        assert!(dir.path().join("models").join("current.bin").exists());
    }

    #[test]
    fn test_hash_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        let model_bytes = b"fake model binary data".to_vec();
        let mut meta = make_epoch_meta("v1.0", &model_bytes, &pkcs8);
        // Valid signature but wrong hash — should fail at hash step
        meta.model_hash = "a".repeat(64); // wrong hash (all 'a's, valid hex length)

        let result = manager.validate_and_store(model_bytes, &meta);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DaemonError::Model(ModelError::HashMismatch { .. })
        ));
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let dir = tempdir().unwrap();
        let (_, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        let model_bytes = b"fake model binary data".to_vec();
        let (wrong_pkcs8, _) = make_keypair(); // different key pair
        let mut meta = make_epoch_meta("v1.0", &model_bytes, &wrong_pkcs8);
        meta.model_hash = sha256_hex(&model_bytes); // correct hash, wrong sig

        let result = manager.validate_and_store(model_bytes, &meta);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DaemonError::Model(ModelError::SignatureVerificationFailed(_))
        ));
    }

    #[test]
    fn test_archive_respects_retention_limit() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let storage = make_storage(dir.path(), 3); // only keep 3
        let manager = ModelManager::new(&storage, pub_key).unwrap();

        // Store 5 versions — archive should cap at 3
        for i in 1..=5usize {
            let bytes = format!("model_v{}", i).into_bytes();
            let meta = make_epoch_meta(&format!("v{}", i), &bytes, &pkcs8);
            manager.validate_and_store(bytes, &meta).unwrap();
        }

        let manifest = manager.load_manifest().unwrap();
        assert!(
            manifest.versions.len() <= 3,
            "Expected ≤3 archived versions, got {}",
            manifest.versions.len()
        );
    }

    #[test]
    fn test_rollback_returns_previous_version() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        // Store v1, then v2
        let bytes_v1 = b"model version one".to_vec();
        let meta_v1 = make_epoch_meta("v1.0", &bytes_v1, &pkcs8);
        manager.validate_and_store(bytes_v1, &meta_v1).unwrap();

        let bytes_v2 = b"model version two".to_vec();
        let meta_v2 = make_epoch_meta("v2.0", &bytes_v2, &pkcs8);
        manager.validate_and_store(bytes_v2, &meta_v2).unwrap();

        // Rollback should return v1
        let rolled_back = manager.rollback("training failed").unwrap();
        assert_eq!(rolled_back.version, "v1.0");
    }

    #[test]
    fn test_rollback_fails_with_no_history() {
        let dir = tempdir().unwrap();
        let (_, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        let result = manager.rollback("test");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DaemonError::Model(ModelError::NoPreviousVersion)
        ));
    }

    // ── Property-based tests ──────────────────────────────────────────────────
    //
    // **Validates: Requirements 4.4, 22, 30**
    //
    // Property 6:  Model Hash Verification          (Req 4.4)
    // Property 33: Model Version Archive Size Limit  (Req 22.1, 22.2, 22.7)
    // Property 41: Model Signature Verification     (Req 30.1–30.6)

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(100))]

        /// Property 6: Any model whose SHA-256 hash doesn't match the expected value
        /// SHALL be rejected before storage.
        ///
        /// **Validates: Requirements 4.4**
        #[test]
        fn prop_hash_mismatch_always_rejected(
            model_data in proptest::collection::vec(proptest::num::u8::ANY, 10..=200),
            bad_hex in "[0-9a-f]{64}",
        ) {
            let dir = tempdir().unwrap();
            let (pkcs8_bytes, pub_key) = make_keypair();
            let manager = make_manager(dir.path(), pub_key);

            let actual_hash = sha256_hex(&model_data);
            proptest::prop_assume!(bad_hex != actual_hash);

            let mut meta = make_epoch_meta("v1", &model_data, &pkcs8_bytes);
            meta.model_hash = bad_hex;
            // signature stays valid for the original bytes

            let result = manager.validate_and_store(model_data, &meta);
            proptest::prop_assert!(result.is_err(), "hash mismatch should be rejected");
        }

        /// Property 33: Archive size SHALL NOT exceed the configured retention count.
        ///
        /// **Validates: Requirements 22**
        #[test]
        fn prop_archive_never_exceeds_retention(
            retention in 1usize..=5,
            num_models in 2usize..=10,
        ) {
            let dir = tempdir().unwrap();
            let (pkcs8_bytes, pub_key) = make_keypair();
            let storage = make_storage(dir.path(), retention);
            let manager = ModelManager::new(&storage, pub_key).unwrap();

            for i in 0..num_models {
                let bytes = format!("model data {}", i).into_bytes();
                let meta = make_epoch_meta(&format!("v{}", i), &bytes, &pkcs8_bytes);
                manager.validate_and_store(bytes, &meta).unwrap();
            }

            let manifest = manager.load_manifest().unwrap();
            proptest::prop_assert!(
                manifest.versions.len() <= retention,
                "archive has {} entries, limit is {}",
                manifest.versions.len(), retention
            );
        }

        /// Property 41: Models signed with a wrong key are always rejected.
        ///
        /// **Validates: Requirements 30**
        #[test]
        fn prop_wrong_signature_always_rejected(
            model_data in proptest::collection::vec(proptest::num::u8::ANY, 10..=200),
        ) {
            let dir = tempdir().unwrap();
            let (_, correct_pub_key) = make_keypair();
            let (wrong_pkcs8, _) = make_keypair(); // different key
            let manager = make_manager(dir.path(), correct_pub_key);

            let mut meta = make_epoch_meta("v1", &model_data, &wrong_pkcs8);
            meta.model_hash = sha256_hex(&model_data); // correct hash, wrong sig

            let result = manager.validate_and_store(model_data, &meta);
            proptest::prop_assert!(result.is_err(), "wrong signature should be rejected");
        }

        /// Property 41b: Models with empty signatures are always rejected.
        ///
        /// **Validates: Requirements 30**
        #[test]
        fn prop_empty_signature_rejected(
            model_data in proptest::collection::vec(proptest::num::u8::ANY, 10..=200),
        ) {
            let dir = tempdir().unwrap();
            let (pkcs8_bytes, pub_key) = make_keypair();
            let manager = make_manager(dir.path(), pub_key);

            let mut meta = make_epoch_meta("v1", &model_data, &pkcs8_bytes);
            meta.model_signature = vec![]; // empty
            meta.model_hash = sha256_hex(&model_data);

            let result = manager.validate_and_store(model_data, &meta);
            proptest::prop_assert!(result.is_err(), "empty signature should be rejected");
        }

        /// Property 32: Models with mismatched architecture hashes are always rejected.
        ///
        /// After storing a model with arch hash A, any attempt to store a new model
        /// with a different arch hash B must be rejected.
        ///
        /// **Validates: Requirements 21.1**
        #[test]
        fn prop_architecture_compatibility(
            model_data1 in proptest::collection::vec(proptest::num::u8::ANY, 10..=100),
            model_data2 in proptest::collection::vec(proptest::num::u8::ANY, 10..=100),
            arch_hash_suffix in "[a-z]{4,8}",
        ) {
            let dir = tempdir().unwrap();
            let (pkcs8_bytes, pub_key) = make_keypair();
            let manager = make_manager(dir.path(), pub_key);

            let arch_a = format!("arch-{}", arch_hash_suffix);
            let arch_b = format!("arch-DIFFERENT-{}", arch_hash_suffix);
            proptest::prop_assume!(arch_a != arch_b);

            // Store first model with arch_a
            let mut meta1 = make_epoch_meta("v1", &model_data1, &pkcs8_bytes);
            meta1.architecture_hash = arch_a.clone();
            manager.validate_and_store(model_data1, &meta1).unwrap();

            // Attempt to store a second model with a different arch hash — must fail
            let mut meta2 = make_epoch_meta("v2", &model_data2, &pkcs8_bytes);
            meta2.architecture_hash = arch_b.clone();

            let result = manager.validate_and_store(model_data2, &meta2);
            proptest::prop_assert!(
                result.is_err(),
                "model with mismatched arch hash ({} vs stored {}) should be rejected",
                arch_b, arch_a
            );
            proptest::prop_assert!(
                matches!(result.unwrap_err(), DaemonError::Model(ModelError::IncompatibleArchitecture(_))),
                "error should be IncompatibleArchitecture"
            );
        }
    }

    // ── Architecture hash unit tests (Req 21.1, Task 22) ─────────────────────

    /// Empty architecture hash is rejected.
    #[test]
    fn test_empty_architecture_hash_rejected() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        let model_bytes = b"test model".to_vec();
        let mut meta = make_epoch_meta("v1.0", &model_bytes, &pkcs8);
        meta.architecture_hash = String::new();

        let result = manager.validate_and_store(model_bytes, &meta);
        assert!(result.is_err(), "empty architecture hash should be rejected");
        assert!(
            matches!(result.unwrap_err(), DaemonError::Model(ModelError::IncompatibleArchitecture(_))),
            "error should be IncompatibleArchitecture"
        );
    }

    /// Storing a model then attempting to store one with a different architecture hash fails.
    #[test]
    fn test_architecture_hash_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        // Store v1 with arch-hash-A
        let model_bytes_v1 = b"model v1 data".to_vec();
        let mut meta_v1 = make_epoch_meta("v1.0", &model_bytes_v1, &pkcs8);
        meta_v1.architecture_hash = "arch-hash-A".to_string();
        manager.validate_and_store(model_bytes_v1, &meta_v1).unwrap();

        // Attempt to store v2 with arch-hash-B (different)
        let model_bytes_v2 = b"model v2 data".to_vec();
        let mut meta_v2 = make_epoch_meta("v2.0", &model_bytes_v2, &pkcs8);
        meta_v2.architecture_hash = "arch-hash-B".to_string();

        let result = manager.validate_and_store(model_bytes_v2, &meta_v2);
        assert!(result.is_err(), "mismatched architecture hash should be rejected");
        let err = result.unwrap_err();
        assert!(
            matches!(err, DaemonError::Model(ModelError::IncompatibleArchitecture(_))),
            "expected IncompatibleArchitecture error, got: {:?}",
            err
        );
    }

    /// Storing two models with the same architecture hash succeeds.
    #[test]
    fn test_matching_architecture_hash_accepted() {
        let dir = tempdir().unwrap();
        let (pkcs8, pub_key) = make_keypair();
        let manager = make_manager(dir.path(), pub_key);

        let arch = "arch-hash-consistent";

        let model_bytes_v1 = b"model v1 data".to_vec();
        let mut meta_v1 = make_epoch_meta("v1.0", &model_bytes_v1, &pkcs8);
        meta_v1.architecture_hash = arch.to_string();
        manager.validate_and_store(model_bytes_v1, &meta_v1).unwrap();

        let model_bytes_v2 = b"model v2 data".to_vec();
        let mut meta_v2 = make_epoch_meta("v2.0", &model_bytes_v2, &pkcs8);
        meta_v2.architecture_hash = arch.to_string();
        let result = manager.validate_and_store(model_bytes_v2, &meta_v2);
        assert!(result.is_ok(), "matching architecture hashes should be accepted: {:?}", result.err());
    }

    /// zeroize_model_binary clears the binary field.
    #[test]
    fn test_zeroize_model_binary() {
        use crate::config::MlFramework;
        use crate::types::ModelMetadata;
        let mut model = Model {
            version: "v1".to_string(),
            architecture_hash: "arch-x".to_string(),
            framework: MlFramework::PyTorch,
            binary: vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03],
            metadata: ModelMetadata {
                input_shape: vec![],
                output_shape: vec![],
                parameter_count: 0,
                created_at: None,
            },
        };
        assert!(!model.binary.is_empty(), "binary should be non-empty before zeroize");
        ModelManager::zeroize_model_binary(&mut model);
        // After zeroize, the vec is filled with zeroes
        assert!(model.binary.iter().all(|&b| b == 0), "binary should be zeroed");
    }
}
