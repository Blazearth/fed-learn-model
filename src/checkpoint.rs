//! Training checkpoint management
//!
//! Implements Requirements: 25
//! Design properties: 34 (checkpoint round-trip preservation)

use std::fs;
use std::path::PathBuf;

use chrono::Utc;

use crate::error::{CheckpointError, DaemonError, Result};
use crate::types::Checkpoint;

// ── CheckpointManager ─────────────────────────────────────────────────────────

/// Manages training checkpoints: save, load, cleanup.
pub struct CheckpointManager {
    /// Directory where checkpoint files are stored.
    checkpoint_dir: PathBuf,
}

impl CheckpointManager {
    /// Create a new CheckpointManager and ensure the directory exists.
    pub fn new(checkpoint_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&checkpoint_dir).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::SaveFailed(format!(
                "cannot create checkpoint directory {}: {e}",
                checkpoint_dir.display()
            )))
        })?;
        Ok(Self { checkpoint_dir })
    }

    /// Save a checkpoint to disk as JSON.
    ///
    /// File is named `{job_id}_{epoch}.json`.
    pub fn save(&self, checkpoint: &Checkpoint) -> Result<()> {
        let filename = format!(
            "{}_{}.json",
            sanitize_id(&checkpoint.job_id),
            checkpoint.epoch
        );
        let path = self.checkpoint_dir.join(&filename);

        let json = serde_json::to_string_pretty(checkpoint).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::SaveFailed(format!(
                "serialize checkpoint: {e}"
            )))
        })?;

        fs::write(&path, json).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::SaveFailed(format!(
                "write checkpoint {}: {e}",
                path.display()
            )))
        })?;

        tracing::debug!(
            job_id = %checkpoint.job_id,
            epoch = checkpoint.epoch,
            path = %path.display(),
            "Checkpoint saved"
        );
        Ok(())
    }

    /// Find the latest checkpoint for a given job_id.
    ///
    /// Scans the checkpoint directory, finds the highest epoch for the job.
    pub fn find_latest(&self, job_id: &str) -> Result<Option<Checkpoint>> {
        let sanitized_id = sanitize_id(job_id);
        let prefix = format!("{}_", sanitized_id);

        let mut candidates: Vec<(u32, PathBuf)> = Vec::new();

        let entries = fs::read_dir(&self.checkpoint_dir).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::LoadFailed(format!(
                "read checkpoint dir: {e}"
            )))
        })?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with(&prefix) && name_str.ends_with(".json") {
                // Extract epoch number from filename: {id}_{epoch}.json
                let without_prefix = &name_str[prefix.len()..];
                let without_suffix = without_prefix.trim_end_matches(".json");
                if let Ok(epoch) = without_suffix.parse::<u32>() {
                    candidates.push((epoch, entry.path()));
                }
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // Find highest epoch
        candidates.sort_by_key(|(epoch, _)| *epoch);
        let (_, latest_path) = candidates.last().unwrap();

        let data = fs::read_to_string(latest_path).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::LoadFailed(format!(
                "read checkpoint file: {e}"
            )))
        })?;

        let checkpoint: Checkpoint = serde_json::from_str(&data).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::Corrupted(format!(
                "parse checkpoint JSON: {e}"
            )))
        })?;

        Ok(Some(checkpoint))
    }

    /// Delete all checkpoints for a given job_id.
    pub fn delete_for_job(&self, job_id: &str) -> Result<()> {
        let sanitized_id = sanitize_id(job_id);
        let prefix = format!("{}_", sanitized_id);

        let entries = fs::read_dir(&self.checkpoint_dir).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::SaveFailed(format!(
                "read checkpoint dir: {e}"
            )))
        })?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with(&prefix) && name_str.ends_with(".json") {
                let path = entry.path();
                fs::remove_file(&path).map_err(|e| {
                    DaemonError::Checkpoint(CheckpointError::SaveFailed(format!(
                        "delete checkpoint {}: {e}",
                        path.display()
                    )))
                })?;
                tracing::debug!(path = %path.display(), "Checkpoint deleted");
            }
        }

        Ok(())
    }

    /// Clean up checkpoints older than `retention_secs` seconds.
    ///
    /// Returns the count of deleted checkpoints.
    pub fn cleanup_old(&self, retention_secs: u64) -> Result<usize> {
        let now = Utc::now();
        let mut deleted = 0usize;

        let entries = fs::read_dir(&self.checkpoint_dir).map_err(|e| {
            DaemonError::Checkpoint(CheckpointError::SaveFailed(format!(
                "read checkpoint dir: {e}"
            )))
        })?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if !name_str.ends_with(".json") {
                continue;
            }

            let path = entry.path();

            // Read the checkpoint to get its timestamp
            let data = match fs::read_to_string(&path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let checkpoint: Checkpoint = match serde_json::from_str(&data) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let age_secs = (now - checkpoint.timestamp).num_seconds().max(0) as u64;

            if age_secs > retention_secs {
                if fs::remove_file(&path).is_ok() {
                    deleted += 1;
                    tracing::debug!(
                        path = %path.display(),
                        age_secs,
                        "Old checkpoint cleaned up"
                    );
                }
            }
        }

        Ok(deleted)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Strip characters unsafe for filesystem filenames from an ID.
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TrainingMetrics;
    use chrono::{Duration, Utc};
    use tempfile::tempdir;

    fn make_checkpoint(job_id: &str, epoch: u32) -> Checkpoint {
        Checkpoint {
            job_id: job_id.to_string(),
            epoch,
            model_state: vec![1u8, 2, 3, 4],
            optimizer_state: vec![5u8, 6, 7],
            metrics: TrainingMetrics {
                loss_history: vec![0.9, 0.7],
                accuracy_history: vec![0.6, 0.8],
                gradient_norms: vec![1.0, 0.8],
                total_time_secs: 30,
            },
            timestamp: Utc::now(),
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    /// test_save_and_load_checkpoint — save, find_latest returns same data
    #[test]
    fn test_save_and_load_checkpoint() {
        let dir = tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf()).unwrap();

        let ckpt = make_checkpoint("job-abc-001", 5);
        manager.save(&ckpt).unwrap();

        let loaded = manager.find_latest("job-abc-001").unwrap();
        assert!(loaded.is_some(), "should find saved checkpoint");
        let loaded = loaded.unwrap();

        assert_eq!(loaded.job_id, ckpt.job_id);
        assert_eq!(loaded.epoch, ckpt.epoch);
        assert_eq!(loaded.model_state, ckpt.model_state);
        assert_eq!(loaded.optimizer_state, ckpt.optimizer_state);
        assert_eq!(loaded.metrics.loss_history, ckpt.metrics.loss_history);
        assert_eq!(loaded.metrics.total_time_secs, ckpt.metrics.total_time_secs);
    }

    /// test_delete_removes_files — save checkpoint, delete_for_job, find_latest returns None
    #[test]
    fn test_delete_removes_files() {
        let dir = tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf()).unwrap();

        let ckpt = make_checkpoint("job-delete-test", 3);
        manager.save(&ckpt).unwrap();

        // Verify it exists
        assert!(manager.find_latest("job-delete-test").unwrap().is_some());

        // Delete
        manager.delete_for_job("job-delete-test").unwrap();

        // Should be gone
        let result = manager.find_latest("job-delete-test").unwrap();
        assert!(result.is_none(), "checkpoint should be gone after deletion");
    }

    /// test_cleanup_old_removes_expired — save old checkpoint, cleanup returns 1
    #[test]
    fn test_cleanup_old_removes_expired() {
        let dir = tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf()).unwrap();

        // Create an "old" checkpoint with timestamp in the past
        let mut old_ckpt = make_checkpoint("job-old", 1);
        old_ckpt.timestamp = Utc::now() - Duration::hours(48); // 2 days old

        manager.save(&old_ckpt).unwrap();

        // Also save a recent one
        let recent = make_checkpoint("job-recent", 1);
        manager.save(&recent).unwrap();

        // Cleanup with 1 hour retention
        let deleted = manager.cleanup_old(3600).unwrap();
        assert_eq!(deleted, 1, "should have deleted 1 old checkpoint");

        // Recent should still exist
        assert!(manager.find_latest("job-recent").unwrap().is_some());
        // Old should be gone
        assert!(manager.find_latest("job-old").unwrap().is_none());
    }

    /// find_latest returns highest-epoch checkpoint when multiple saved
    #[test]
    fn test_find_latest_returns_highest_epoch() {
        let dir = tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf()).unwrap();

        for epoch in [1u32, 3, 2] {
            manager.save(&make_checkpoint("job-multi", epoch)).unwrap();
        }

        let latest = manager.find_latest("job-multi").unwrap().unwrap();
        assert_eq!(latest.epoch, 3, "should return epoch 3 (highest)");
    }

    // ── Property-based tests ──────────────────────────────────────────────────
    //
    // **Validates: Requirements 25**
    //
    // Property 34: Checkpoint Round-Trip Preservation

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(100))]

        /// Property 34: Random job_id/epoch/state → save → find_latest → fields match.
        ///
        /// **Validates: Requirements 25**
        #[test]
        fn prop_checkpoint_round_trip(
            job_id_suffix in "[a-z0-9]{4,12}",
            epoch in 1u32..=1000,
            model_bytes in proptest::collection::vec(proptest::num::u8::ANY, 1..=64),
            opt_bytes in proptest::collection::vec(proptest::num::u8::ANY, 1..=32),
        ) {
            let dir = tempdir().unwrap();
            let manager = CheckpointManager::new(dir.path().to_path_buf()).unwrap();

            let job_id = format!("prop-{}", job_id_suffix);
            let ckpt = Checkpoint {
                job_id: job_id.clone(),
                epoch,
                model_state: model_bytes.clone(),
                optimizer_state: opt_bytes.clone(),
                metrics: TrainingMetrics {
                    loss_history: vec![0.5],
                    accuracy_history: vec![0.8],
                    gradient_norms: vec![1.0],
                    total_time_secs: 10,
                },
                timestamp: Utc::now(),
            };

            manager.save(&ckpt).unwrap();
            let loaded = manager.find_latest(&job_id).unwrap();

            proptest::prop_assert!(loaded.is_some(), "checkpoint should be found after save");
            let loaded = loaded.unwrap();

            proptest::prop_assert_eq!(&loaded.job_id, &job_id);
            proptest::prop_assert_eq!(loaded.epoch, epoch);
            proptest::prop_assert_eq!(&loaded.model_state, &model_bytes);
            proptest::prop_assert_eq!(&loaded.optimizer_state, &opt_bytes);
        }
    }
}
