//! Audit logging and tamper-evident log management

use chrono::Utc;
use ring::digest::{digest, SHA256};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::{AuditError, Result};
use crate::types::{AuditEvent, LogEntry, LogSeverity};

/// Audit engine for tamper-evident logging
pub struct AuditEngine {
    /// Path to audit log file
    log_path: PathBuf,
    /// Last entry hash for hash chaining
    last_hash: Arc<Mutex<Vec<u8>>>,
    /// Signing key for log entries (optional)
    signing_key: Option<Ed25519KeyPair>,
    /// Enable tamper-evident features
    tamper_evident: bool,
    /// Entry counter
    entry_counter: Arc<Mutex<u64>>,
}

impl AuditEngine {
    /// Create new audit engine
    pub fn new(
        log_path: PathBuf,
        tamper_evident: bool,
        signing_key_bytes: Option<&[u8]>,
    ) -> Result<Self> {
        // Initialize signing key if provided
        let signing_key = if let Some(key_bytes) = signing_key_bytes {
            if key_bytes.len() != 32 {
                return Err(AuditError::SigningFailed(
                    "signing key must be 32 bytes".to_string(),
                )
                .into());
            }
            Some(
                Ed25519KeyPair::from_seed_unchecked(key_bytes)
                    .map_err(|e| AuditError::SigningFailed(e.to_string()))?,
            )
        } else if tamper_evident {
            return Err(AuditError::SigningFailed(
                "signing key required when tamper_evident is enabled".to_string(),
            )
            .into());
        } else {
            None
        };

        // Initialize last hash (genesis hash)
        let genesis_hash = vec![0u8; 32];

        // Get last entry number from existing log
        let entry_counter = Self::get_last_entry_number(&log_path)?;

        Ok(Self {
            log_path,
            last_hash: Arc::new(Mutex::new(genesis_hash)),
            signing_key,
            tamper_evident,
            entry_counter: Arc::new(Mutex::new(entry_counter)),
        })
    }

    /// Log an audit event
    pub fn log_event(
        &self,
        event_type: impl Into<String>,
        severity: LogSeverity,
        message: impl Into<String>,
        context: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let event = AuditEvent {
            timestamp: Utc::now(),
            event_type: event_type.into(),
            severity,
            message: message.into(),
            context,
        };

        if self.tamper_evident {
            self.log_with_chain(event)
        } else {
            self.log_simple(event)
        }
    }

    /// Log event with tamper-evident hash chain
    fn log_with_chain(&self, event: AuditEvent) -> Result<()> {
        let mut counter = self.entry_counter.lock().unwrap();
        let mut last_hash = self.last_hash.lock().unwrap();

        *counter += 1;
        let entry_number = *counter;

        // Serialize event
        let event_json = serde_json::to_string(&event)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        // Compute entry hash (previous_hash + entry_number + event)
        let mut hash_input = last_hash.clone();
        hash_input.extend_from_slice(&entry_number.to_le_bytes());
        hash_input.extend_from_slice(event_json.as_bytes());

        let entry_hash = digest(&SHA256, &hash_input);
        let entry_hash_bytes = entry_hash.as_ref().to_vec();

        // Sign entry hash
        let signature = if let Some(ref key) = self.signing_key {
            key.sign(&entry_hash_bytes).as_ref().to_vec()
        } else {
            vec![]
        };

        // Create log entry
        let log_entry = LogEntry {
            entry_number,
            previous_hash: last_hash.clone(),
            event,
            entry_hash: entry_hash_bytes.clone(),
            signature,
        };

        // Write to file
        self.write_entry(&log_entry)?;

        // Update last hash
        *last_hash = entry_hash_bytes;

        Ok(())
    }

    /// Log event without hash chain (simple mode)
    fn log_simple(&self, event: AuditEvent) -> Result<()> {
        let event_json = serde_json::to_string(&event)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        writeln!(file, "{}", event_json)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        Ok(())
    }

    /// Write log entry to file
    fn write_entry(&self, entry: &LogEntry) -> Result<()> {
        let entry_json = serde_json::to_string(entry)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        writeln!(file, "{}", entry_json)
            .map_err(|e| AuditError::WriteFailed(e.to_string()))?;

        Ok(())
    }

    /// Verify log integrity
    pub fn verify_log_integrity(&self) -> Result<bool> {
        if !self.tamper_evident {
            return Ok(true); // Simple mode has no integrity checks
        }

        let file = File::open(&self.log_path)
            .map_err(|e| AuditError::IntegrityCheckFailed(e.to_string()))?;

        let reader = BufReader::new(file);
        let mut previous_hash = vec![0u8; 32]; // Genesis hash
        let mut entry_num = 0u64;

        for (line_num, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| AuditError::IntegrityCheckFailed(e.to_string()))?;

            let entry: LogEntry = serde_json::from_str(&line)
                .map_err(|e| AuditError::IntegrityCheckFailed(e.to_string()))?;

            entry_num += 1;

            // Verify entry number
            if entry.entry_number != entry_num {
                return Err(AuditError::TamperingDetected(line_num).into());
            }

            // Verify previous hash matches
            if entry.previous_hash != previous_hash {
                return Err(AuditError::TamperingDetected(line_num).into());
            }

            // Recompute entry hash
            let event_json = serde_json::to_string(&entry.event)
                .map_err(|e| AuditError::IntegrityCheckFailed(e.to_string()))?;

            let mut hash_input = previous_hash.clone();
            hash_input.extend_from_slice(&entry.entry_number.to_le_bytes());
            hash_input.extend_from_slice(event_json.as_bytes());

            let computed_hash = digest(&SHA256, &hash_input);

            if computed_hash.as_ref() != entry.entry_hash.as_slice() {
                return Err(AuditError::TamperingDetected(line_num).into());
            }

            // Verify signature if present
            if !entry.signature.is_empty() {
                if let Some(ref key) = self.signing_key {
                    let public_key = key.public_key();
                    let signature = ring::signature::UnparsedPublicKey::new(
                        &ring::signature::ED25519,
                        public_key.as_ref(),
                    );

                    signature
                        .verify(&entry.entry_hash, &entry.signature)
                        .map_err(|_| AuditError::TamperingDetected(line_num))?;
                }
            }

            previous_hash = entry.entry_hash.clone();
        }

        Ok(true)
    }

    /// Get last entry number from existing log file
    fn get_last_entry_number(log_path: &Path) -> Result<u64> {
        if !log_path.exists() {
            return Ok(0);
        }

        let file = File::open(log_path).map_err(|_| AuditError::WriteFailed(
            "failed to open log file".to_string(),
        ))?;

        let reader = BufReader::new(file);
        let mut last_num = 0u64;

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
                    last_num = entry.entry_number;
                }
            }
        }

        Ok(last_num)
    }

    /// Anchor audit log hash to blockchain (stub for now)
    pub async fn anchor_to_fabric(&self) -> Result<String> {
        // This is a stub - actual implementation would:
        // 1. Get aggregate hash of recent entries
        // 2. Submit transaction to Hyperledger Fabric
        // 3. Return transaction ID

        // For now, compute aggregate hash
        let file = File::open(&self.log_path)
            .map_err(|e| AuditError::BlockchainAnchoringFailed(e.to_string()))?;

        let reader = BufReader::new(file);
        let mut aggregate_hash_input = Vec::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                aggregate_hash_input.extend_from_slice(line.as_bytes());
            }
        }

        let aggregate_hash = digest(&SHA256, &aggregate_hash_input);
        let hash_hex = hex::encode(aggregate_hash.as_ref());

        tracing::info!(
            "Anchoring audit log to Fabric (stub): hash={}",
            hash_hex
        );

        // Return mock transaction ID
        Ok(format!("fabric-tx-{}", hash_hex[..16].to_string()))
    }
}

// Helper function to convert bytes to hex string
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_key() -> [u8; 32] {
        [1u8; 32] // Simple test key
    }

    #[test]
    fn test_simple_logging() {
        let temp_file = NamedTempFile::new().unwrap();
        let engine = AuditEngine::new(temp_file.path().to_path_buf(), false, None).unwrap();

        let mut context = HashMap::new();
        context.insert("test".to_string(), serde_json::json!("value"));

        let result = engine.log_event("test_event", LogSeverity::Info, "test message", context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tamper_evident_logging() {
        let temp_file = NamedTempFile::new().unwrap();
        let key = create_test_key();
        let engine = AuditEngine::new(temp_file.path().to_path_buf(), true, Some(&key)).unwrap();

        let mut context = HashMap::new();
        context.insert("test".to_string(), serde_json::json!("value"));

        // Log multiple events
        for i in 0..5 {
            let result = engine.log_event(
                format!("event_{}", i),
                LogSeverity::Info,
                format!("message {}", i),
                context.clone(),
            );
            assert!(result.is_ok());
        }

        // Verify integrity
        let integrity = engine.verify_log_integrity();
        assert!(integrity.is_ok());
        assert!(integrity.unwrap());
    }

    #[test]
    fn test_tampering_detection() {
        let temp_file = NamedTempFile::new().unwrap();
        let key = create_test_key();
        let engine = AuditEngine::new(temp_file.path().to_path_buf(), true, Some(&key)).unwrap();

        let mut context = HashMap::new();
        context.insert("test".to_string(), serde_json::json!("value"));

        // Log events
        for i in 0..3 {
            engine
                .log_event(
                    format!("event_{}", i),
                    LogSeverity::Info,
                    format!("message {}", i),
                    context.clone(),
                )
                .unwrap();
        }

        // Tamper with the log file
        let mut contents = std::fs::read_to_string(temp_file.path()).unwrap();
        contents = contents.replace("message 1", "tampered message");
        std::fs::write(temp_file.path(), contents).unwrap();

        // Verify integrity should fail
        let integrity = engine.verify_log_integrity();
        assert!(integrity.is_err());
    }
}
