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
