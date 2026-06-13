//! Core data types for federated learning

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::MlFramework;

/// Model artifact with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// Model version identifier
    pub version: String,

    /// Architecture hash for compatibility checking
    pub architecture_hash: String,

    /// ML framework
    pub framework: MlFramework,

    /// Model binary data
    #[serde(skip)]
    pub binary: Vec<u8>,

    /// Model metadata
    pub metadata: ModelMetadata,
}

/// Model metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    /// Input tensor shape
    pub input_shape: Vec<usize>,

    /// Output tensor shape
    pub output_shape: Vec<usize>,

    /// Total parameter count
    pub parameter_count: usize,

    /// Model creation timestamp
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// Epoch metadata from coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochMetadata {
    /// Current epoch number
    pub epoch_number: u64,

    /// Model identifier
    pub model_id: String,

    /// Model version
    pub model_version: String,

    /// SHA-256 hash of model binary
    pub model_hash: String,

    /// Model signature (Ed25519)
    pub model_signature: Vec<u8>,

    /// Architecture hash for compatibility
    pub architecture_hash: String,

    /// FedProx mu parameter
    pub fedprox_mu: f32,

    /// Privacy budget epsilon
    pub privacy_epsilon: f64,

    /// Privacy budget delta
    pub privacy_delta: f64,

    /// Secure aggregation participants
    pub secure_agg_participants: Vec<ParticipantInfo>,

    /// Secure aggregation threshold
    pub secure_agg_threshold: usize,

    /// Drift alerts from previous rounds (optional)
    #[serde(default)]
    pub drift_alerts: Vec<DriftAlert>,

    /// Expected dataset schema (optional)
    #[serde(default)]
    pub dataset_schema: Option<DatasetSchema>,
}

/// Participant information for secure aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantInfo {
    /// Organization ID
    pub org_id: String,

    /// Public key for secure aggregation
    pub public_key: Vec<u8>,
}

/// Model update computed during training
#[derive(Debug, Clone)]
pub struct ModelUpdate {
    /// Gradient tensors (conceptual - actual representation depends on framework)
    pub gradients: Vec<Vec<f32>>,

    /// Update metadata
    pub metadata: UpdateMetadata,
}

/// Metadata about a model update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMetadata {
    /// Number of training samples used
    pub sample_count: usize,

    /// Final training loss
    pub training_loss: f32,

    /// Final training accuracy
    pub training_accuracy: f32,

    /// Gradient L2 norm
    pub gradient_norm: f32,

    /// Training duration in seconds
    pub epoch_duration_secs: u64,

    /// Privacy parameters applied (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy_params: Option<PrivacyParameters>,
}

/// Privacy parameters applied to an update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyParameters {
    /// Epsilon value used
    pub epsilon: f64,

    /// Delta value used
    pub delta: f64,

    /// Clipping threshold
    pub clip_threshold: f32,

    /// Noise scale applied
    pub noise_scale: f64,
}

/// Training metrics collected during a round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingMetrics {
    /// Loss history per epoch
    pub loss_history: Vec<f32>,

    /// Accuracy history per epoch
    pub accuracy_history: Vec<f32>,

    /// Gradient norms per epoch
    pub gradient_norms: Vec<f32>,

    /// Total training time
    pub total_time_secs: u64,
}

/// Dataset information and statistics
#[derive(Debug, Clone)]
pub struct Dataset {
    /// Feature column names
    pub features: Vec<String>,

    /// Label column name
    pub labels: String,

    /// Number of rows
    pub row_count: usize,

    /// Dataset statistics
    pub statistics: DataStatistics,

    /// Raw data (conceptual - actual storage depends on implementation)
    #[allow(dead_code)]
    pub(crate) data: Vec<u8>,
}

/// Statistical information about a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStatistics {
    /// Mean of each feature
    pub feature_means: Vec<f64>,

    /// Standard deviation of each feature
    pub feature_stddevs: Vec<f64>,

    /// Label distribution (class -> count)
    pub label_distribution: HashMap<String, usize>,

    /// Feature names
    pub feature_names: Vec<String>,
}

/// Expected dataset schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSchema {
    /// Feature column definitions
    pub features: Vec<FeatureSchema>,

    /// Label column definition
    pub label: ColumnSchema,

    /// Schema version
    pub version: String,
}

/// Feature schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSchema {
    /// Feature name
    pub name: String,

    /// Feature data type
    pub dtype: DataType,

    /// Whether NULL values are allowed
    #[serde(default)]
    pub nullable: bool,
}

/// Column schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    /// Column name
    pub name: String,

    /// Column data type
    pub dtype: DataType,

    /// Whether NULL values are allowed
    #[serde(default)]
    pub nullable: bool,
}

/// Supported data types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Int32,
    Int64,
    Float32,
    Float64,
    String,
    Boolean,
    DateTime,
}

/// Drift alert from coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAlert {
    /// Feature name affected by drift
    pub feature_name: String,

    /// Drift metric value
    pub drift_score: f64,

    /// Alert severity (low, medium, high)
    pub severity: DriftSeverity,

    /// Human-readable message
    pub message: String,
}

/// Drift alert severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DriftSeverity {
    Low,
    Medium,
    High,
}

/// Resource usage metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetrics {
    /// CPU utilization percentage
    pub cpu_percent: f32,

    /// RAM usage in GB
    pub ram_gb: f32,

    /// Disk usage in GB
    pub disk_gb: f32,

    /// GPU memory usage in GB (if applicable)
    pub gpu_memory_gb: Option<f32>,

    /// Timestamp of measurement
    pub timestamp: DateTime<Utc>,
}

/// Drift metrics for a training round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftMetrics {
    /// Per-feature drift scores
    pub feature_drift: HashMap<String, f64>,

    /// Overall drift score
    pub overall_drift: f64,

    /// Concept drift detected
    pub concept_drift_detected: bool,

    /// Timestamp of computation
    pub timestamp: DateTime<Utc>,
}

/// Explainability artifact metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainabilityArtifact {
    /// Model ID
    pub model_id: String,

    /// Epoch number
    pub epoch_number: u64,

    /// SHAP values file path (if applicable)
    pub shap_values_path: Option<String>,

    /// Feature importance scores
    pub feature_importance: HashMap<String, f64>,

    /// Summary report path
    pub summary_report_path: String,

    /// Generation timestamp
    pub timestamp: DateTime<Utc>,
}

/// Training job state for scheduler
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
}

/// Training job metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingJob {
    /// Job identifier
    pub job_id: String,

    /// Model ID
    pub model_id: String,

    /// Epoch number
    pub epoch_number: u64,

    /// Job priority
    pub priority: u8,

    /// Current state
    pub state: JobState,

    /// Resource allocation
    pub resources: ResourceAllocation,

    /// Start time
    pub started_at: Option<DateTime<Utc>>,

    /// Completion time
    pub completed_at: Option<DateTime<Utc>>,
}

/// Resource allocation for a job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    /// Allocated CPU percentage
    pub cpu_percent: f32,

    /// Allocated RAM in GB
    pub ram_gb: f32,

    /// Allocated GPU memory in GB (if applicable)
    pub gpu_memory_gb: Option<f32>,
}

/// Checkpoint data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Job ID
    pub job_id: String,

    /// Current epoch number
    pub epoch: u32,

    /// Model state (serialized)
    pub model_state: Vec<u8>,

    /// Optimizer state (serialized)
    pub optimizer_state: Vec<u8>,

    /// Training metrics at checkpoint
    pub metrics: TrainingMetrics,

    /// Checkpoint timestamp
    pub timestamp: DateTime<Utc>,
}

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Event timestamp
    pub timestamp: DateTime<Utc>,

    /// Event type
    pub event_type: String,

    /// Severity level
    pub severity: LogSeverity,

    /// Event message
    pub message: String,

    /// Additional context
    #[serde(flatten)]
    pub context: HashMap<String, serde_json::Value>,
}

/// Log severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogSeverity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Critical,
}

/// Tamper-evident log entry with hash chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Sequential entry number
    pub entry_number: u64,

    /// Hash of previous entry
    pub previous_hash: Vec<u8>,

    /// Audit event
    pub event: AuditEvent,

    /// Hash of this entry
    pub entry_hash: Vec<u8>,

    /// Signature of entry hash
    pub signature: Vec<u8>,
}

// ── Serialization tests (Task 2.6) ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::config::MlFramework;

    fn make_storage_config() -> crate::config::StorageConfig {
        crate::config::StorageConfig {
            working_dir: std::path::PathBuf::from("/tmp/test"),
            model_dir: std::path::PathBuf::from("/tmp/test/models"),
            checkpoint_dir: std::path::PathBuf::from("/tmp/test/checkpoints"),
            audit_log_path: std::path::PathBuf::from("/tmp/test/audit.log"),
            model_retention_count: 5,
            explainability_dir: None,
        }
    }

    /// Round-trip: Configuration → TOML → Configuration
    #[test]
    fn test_configuration_toml_round_trip() {
        use crate::config::*;

        let config = Configuration {
            organization_id: "org-test-123".to_string(),
            coordinator: CoordinatorConfig {
                base_url: "https://coordinator.example.com".to_string(),
                poll_interval_secs: 60,
                max_backoff_secs: 300,
                request_timeout_secs: 30,
                max_retries: 5,
            },
            certificates: CertificateConfig {
                cert_path: std::path::PathBuf::from("/etc/certs/client.pem"),
                cert_dir: std::path::PathBuf::from("/etc/certs"),
                ca_bundle_path: std::path::PathBuf::from("/etc/certs/ca.pem"),
                key_storage: KeyStorageConfig::Tpm {
                    device_path: "/dev/tpm0".to_string(),
                },
                rotation_warning_days: 30,
                check_interval_secs: 3600,
            },
            training: TrainingConfig {
                local_epochs: 5,
                fedprox_mu: 0.01,
                checkpoint_interval_secs: 600,
                checkpoint_retention_secs: 86400,
                framework: MlFramework::PyTorch,
                loss_tolerance_percent: 20.0,
                min_accuracy: None,
                max_gradient_norm: 10.0,
            },
            privacy: PrivacyConfig {
                enabled: true,
                epsilon: 1.0,
                delta: 1e-5,
                clip_threshold: 1.0,
            },
            secure_aggregation: SecureAggConfig {
                enabled: true,
                dropout_recovery: true,
                threshold: Some(3),
            },
            resources: ResourceConfig {
                max_cpu_percent: 80.0,
                max_ram_gb: 8.0,
                max_disk_gb: 100.0,
                max_gpu_memory_gb: None,
                warning_threshold_percent: 80.0,
            },
            storage: make_storage_config(),
            logging: LoggingConfig {
                level: "info".to_string(),
                log_file: std::path::PathBuf::from("/var/log/fl.log"),
                json_format: true,
                tamper_evident: false,
                signing_key: None,
                blockchain_anchoring: false,
                anchoring_interval_secs: 3600,
            },
            network: NetworkConfig {
                max_concurrent_requests: 10,
                connection_pooling: true,
                pool_idle_timeout_secs: 90,
                stream_threshold_bytes: 10 * 1024 * 1024,
            },
            models: vec![],
            attestation: None,
            time_sync: None,
        };

        // Serialize to TOML
        let toml_str = toml::to_string(&config).expect("serialize to TOML");
        assert!(!toml_str.is_empty(), "TOML should not be empty");

        // Deserialize back
        let restored: Configuration = toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(restored.organization_id, config.organization_id);
        assert_eq!(restored.coordinator.base_url, config.coordinator.base_url);
        assert_eq!(restored.training.local_epochs, config.training.local_epochs);
        assert_eq!(restored.privacy.epsilon, config.privacy.epsilon);
        assert_eq!(restored.storage.model_retention_count, config.storage.model_retention_count);
        assert_eq!(restored.storage.model_dir, config.storage.model_dir);
    }

    /// Round-trip: EpochMetadata → JSON → EpochMetadata
    #[test]
    fn test_epoch_metadata_json_round_trip() {
        let meta = EpochMetadata {
            epoch_number: 42,
            model_id: "fraud-detection-v2".to_string(),
            model_version: "v2.1.0".to_string(),
            model_hash: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            model_signature: vec![1, 2, 3, 4, 5],
            architecture_hash: "arch-hash-xyz".to_string(),
            fedprox_mu: 0.01,
            privacy_epsilon: 1.0,
            privacy_delta: 1e-5,
            secure_agg_participants: vec![
                ParticipantInfo {
                    org_id: "org-a".to_string(),
                    public_key: vec![10, 20, 30],
                }
            ],
            secure_agg_threshold: 2,
            drift_alerts: vec![
                DriftAlert {
                    feature_name: "age".to_string(),
                    drift_score: 0.42,
                    severity: DriftSeverity::Medium,
                    message: "Moderate drift detected".to_string(),
                }
            ],
            dataset_schema: Some(DatasetSchema {
                features: vec![
                    FeatureSchema {
                        name: "age".to_string(),
                        dtype: DataType::Float32,
                        nullable: false,
                    }
                ],
                label: ColumnSchema {
                    name: "label".to_string(),
                    dtype: DataType::Int32,
                    nullable: false,
                },
                version: "1.0".to_string(),
            }),
        };

        let json = serde_json::to_string(&meta).expect("serialize EpochMetadata");
        assert!(!json.is_empty());

        let restored: EpochMetadata = serde_json::from_str(&json).expect("deserialize EpochMetadata");
        assert_eq!(restored.epoch_number, meta.epoch_number);
        assert_eq!(restored.model_id, meta.model_id);
        assert_eq!(restored.model_version, meta.model_version);
        assert_eq!(restored.model_hash, meta.model_hash);
        assert_eq!(restored.model_signature, meta.model_signature);
        assert_eq!(restored.fedprox_mu, meta.fedprox_mu);
        assert_eq!(restored.privacy_epsilon, meta.privacy_epsilon);
        assert_eq!(restored.secure_agg_participants.len(), 1);
        assert_eq!(restored.drift_alerts.len(), 1);
        assert_eq!(restored.drift_alerts[0].drift_score, 0.42);
        assert!(restored.dataset_schema.is_some());
    }

    /// Round-trip: Model → JSON → Model (binary field is skipped by serde)
    #[test]
    fn test_model_json_serialization_binary_skipped() {
        let model = Model {
            version: "v1.2.3".to_string(),
            architecture_hash: "arch-abc".to_string(),
            framework: MlFramework::PyTorch,
            binary: vec![0xDE, 0xAD, 0xBE, 0xEF], // should be skipped
            metadata: ModelMetadata {
                input_shape: vec![1, 28, 28],
                output_shape: vec![10],
                parameter_count: 100_000,
                created_at: Some(Utc::now()),
            },
        };

        let json = serde_json::to_string(&model).expect("serialize Model");

        // Binary field is #[serde(skip)] so it should not appear in JSON
        assert!(!json.contains("deadbeef"), "binary should not be in JSON");
        assert!(!json.contains("binary"), "binary field name should not be in JSON");
        assert!(json.contains("v1.2.3"), "version should be in JSON");
        assert!(json.contains("arch-abc"), "architecture_hash should be in JSON");
        assert!(json.contains("100000"), "parameter_count should be in JSON");

        let restored: Model = serde_json::from_str(&json).expect("deserialize Model");
        assert_eq!(restored.version, model.version);
        assert_eq!(restored.architecture_hash, model.architecture_hash);
        assert_eq!(restored.metadata.parameter_count, model.metadata.parameter_count);
        assert_eq!(restored.metadata.input_shape, model.metadata.input_shape);
        // binary is empty after deserialization since it's skipped
        assert!(restored.binary.is_empty(), "binary should be empty after deserialization");
    }

    /// Round-trip: LogEntry → JSON → LogEntry
    #[test]
    fn test_log_entry_json_round_trip() {
        let entry = LogEntry {
            entry_number: 7,
            previous_hash: vec![0xAA, 0xBB, 0xCC],
            event: AuditEvent {
                timestamp: Utc::now(),
                event_type: "model_validated".to_string(),
                severity: LogSeverity::Info,
                message: "Model signature verified successfully".to_string(),
                context: {
                    let mut m = HashMap::new();
                    m.insert("model_id".to_string(), serde_json::Value::String("fraud-v1".to_string()));
                    m.insert("epoch".to_string(), serde_json::Value::Number(serde_json::Number::from(5)));
                    m
                },
            },
            entry_hash: vec![0x11, 0x22, 0x33, 0x44],
            signature: vec![0xFF, 0xEE, 0xDD],
        };

        let json = serde_json::to_string(&entry).expect("serialize LogEntry");
        assert!(!json.is_empty());

        let restored: LogEntry = serde_json::from_str(&json).expect("deserialize LogEntry");
        assert_eq!(restored.entry_number, entry.entry_number);
        assert_eq!(restored.previous_hash, entry.previous_hash);
        assert_eq!(restored.entry_hash, entry.entry_hash);
        assert_eq!(restored.signature, entry.signature);
        assert_eq!(restored.event.event_type, entry.event.event_type);
        assert_eq!(restored.event.severity, entry.event.severity);
        assert_eq!(restored.event.message, entry.event.message);
        // context flattened fields should survive the round-trip
        assert_eq!(
            restored.event.context.get("model_id"),
            Some(&serde_json::Value::String("fraud-v1".to_string()))
        );
    }

    /// AuditEvent serializes to valid JSON containing all required fields:
    /// timestamp, event_type, severity, message  (Requirement 11.8)
    #[test]
    fn test_audit_event_json_required_fields() {
        let event = AuditEvent {
            timestamp: Utc::now(),
            event_type: "training_round_started".to_string(),
            severity: LogSeverity::Info,
            message: "Training round 5 started".to_string(),
            context: HashMap::new(),
        };

        let json = serde_json::to_string(&event).expect("serialize AuditEvent");

        // Parse into a generic Value to inspect field names
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse AuditEvent JSON");
        let obj = value.as_object().expect("AuditEvent should be a JSON object");

        assert!(obj.contains_key("timestamp"), "JSON must contain 'timestamp'");
        assert!(obj.contains_key("event_type"), "JSON must contain 'event_type'");
        assert!(obj.contains_key("severity"), "JSON must contain 'severity'");
        assert!(obj.contains_key("message"), "JSON must contain 'message'");

        // Verify value correctness
        assert_eq!(obj["event_type"], "training_round_started");
        assert_eq!(obj["severity"], "info");
        assert_eq!(obj["message"], "Training round 5 started");
    }

    /// AuditEvent with context fields flattened into JSON output (Requirement 11.8)
    #[test]
    fn test_audit_event_context_flattened_into_json() {
        let mut context = HashMap::new();
        context.insert("epoch".to_string(), serde_json::Value::Number(serde_json::Number::from(3)));
        context.insert("org_id".to_string(), serde_json::Value::String("org-xyz".to_string()));

        let event = AuditEvent {
            timestamp: Utc::now(),
            event_type: "update_uploaded".to_string(),
            severity: LogSeverity::Info,
            message: "Protected update uploaded".to_string(),
            context,
        };

        let json = serde_json::to_string(&event).expect("serialize AuditEvent with context");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object().unwrap();

        // Context fields are flattened via #[serde(flatten)], so they appear at the top level
        assert!(obj.contains_key("epoch"), "flattened 'epoch' should be top-level");
        assert!(obj.contains_key("org_id"), "flattened 'org_id' should be top-level");
        assert_eq!(obj["epoch"], 3);
        assert_eq!(obj["org_id"], "org-xyz");
    }

    /// Round-trip: TrainingMetrics → JSON → TrainingMetrics (Requirement 1.8)
    #[test]
    fn test_training_metrics_json_round_trip() {
        let metrics = TrainingMetrics {
            loss_history: vec![0.9, 0.7, 0.5, 0.3],
            accuracy_history: vec![0.55, 0.70, 0.82, 0.91],
            gradient_norms: vec![1.2, 0.9, 0.7, 0.5],
            total_time_secs: 120,
        };

        let json = serde_json::to_string(&metrics).expect("serialize TrainingMetrics");
        assert!(!json.is_empty());

        let restored: TrainingMetrics = serde_json::from_str(&json).expect("deserialize TrainingMetrics");
        assert_eq!(restored.loss_history, metrics.loss_history);
        assert_eq!(restored.accuracy_history, metrics.accuracy_history);
        assert_eq!(restored.gradient_norms, metrics.gradient_norms);
        assert_eq!(restored.total_time_secs, metrics.total_time_secs);
    }

    /// Round-trip: Checkpoint → JSON → Checkpoint (Requirements 1.7, 1.8)
    #[test]
    fn test_checkpoint_json_round_trip() {
        let checkpoint = Checkpoint {
            job_id: "job-abc-001".to_string(),
            epoch: 3,
            model_state: vec![0x01, 0x02, 0x03, 0x04],
            optimizer_state: vec![0x0A, 0x0B, 0x0C],
            metrics: TrainingMetrics {
                loss_history: vec![0.8, 0.6],
                accuracy_history: vec![0.60, 0.75],
                gradient_norms: vec![1.1, 0.8],
                total_time_secs: 60,
            },
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&checkpoint).expect("serialize Checkpoint");
        assert!(!json.is_empty());

        let restored: Checkpoint = serde_json::from_str(&json).expect("deserialize Checkpoint");
        assert_eq!(restored.job_id, checkpoint.job_id);
        assert_eq!(restored.epoch, checkpoint.epoch);
        assert_eq!(restored.model_state, checkpoint.model_state);
        assert_eq!(restored.optimizer_state, checkpoint.optimizer_state);
        assert_eq!(restored.metrics.loss_history, checkpoint.metrics.loss_history);
        assert_eq!(restored.metrics.total_time_secs, checkpoint.metrics.total_time_secs);
    }

    /// Error messages include structured context (Requirement 1.8, 11.8)
    #[test]
    fn test_error_message_formatting() {
        use crate::error::{DaemonError, ConfigError, TrainingError, ModelError};

        // ConfigError::MissingField includes the field name
        let err = DaemonError::Config(ConfigError::MissingField("organization_id".to_string()));
        let msg = err.to_string();
        assert!(msg.contains("organization_id"), "error message should contain field name: {msg}");

        // ConfigError::InvalidValue includes field and reason
        let err = DaemonError::Config(ConfigError::InvalidValue {
            field: "privacy.epsilon".to_string(),
            reason: "must be positive".to_string(),
        });
        let msg = err.to_string();
        assert!(msg.contains("privacy.epsilon"), "error message should contain field: {msg}");
        assert!(msg.contains("must be positive"), "error message should contain reason: {msg}");

        // TrainingError::LossOutsideTolerance includes numeric context
        let err = DaemonError::Training(TrainingError::LossOutsideTolerance {
            local: 0.85,
            global: 0.50,
            tolerance: 20.0,
        });
        let msg = err.to_string();
        assert!(msg.contains("0.85") || msg.contains("local"), "should include local loss: {msg}");
        assert!(msg.contains("20") || msg.contains("tolerance"), "should include tolerance: {msg}");

        // ModelError::HashMismatch includes expected and actual values
        let err = DaemonError::Model(ModelError::HashMismatch {
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        });
        let msg = err.to_string();
        assert!(msg.contains("abc123"), "should include expected hash: {msg}");
        assert!(msg.contains("def456"), "should include actual hash: {msg}");
    }
}
