//! Configuration structures and management

pub mod manager;

pub use manager::ConfigManager;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for the daemon
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Configuration {
    /// Unique identifier for this organization
    pub organization_id: String,

    /// Cloud coordinator connection settings
    pub coordinator: CoordinatorConfig,

    /// Certificate and key management settings
    pub certificates: CertificateConfig,

    /// Training configuration
    pub training: TrainingConfig,

    /// Privacy protection settings
    pub privacy: PrivacyConfig,

    /// Secure aggregation settings
    pub secure_aggregation: SecureAggConfig,

    /// Resource limits and quotas
    pub resources: ResourceConfig,

    /// Storage paths and settings
    pub storage: StorageConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// Network settings
    pub network: NetworkConfig,

    /// Model configurations
    pub models: Vec<ModelConfig>,

    /// Attestation settings (optional)
    #[serde(default)]
    pub attestation: Option<AttestationConfig>,

    /// Time synchronization settings (optional)
    #[serde(default)]
    pub time_sync: Option<TimeSyncConfig>,
}

/// Cloud coordinator connection configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoordinatorConfig {
    /// Base URL of the coordinator API
    pub base_url: String,

    /// Polling interval in seconds
    pub poll_interval_secs: u64,

    /// Maximum backoff interval in seconds
    pub max_backoff_secs: u64,

    /// Request timeout in seconds
    pub request_timeout_secs: u64,

    /// Maximum retry count for failed requests
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

/// Certificate and key management configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CertificateConfig {
    /// Path to organization certificate
    pub cert_path: PathBuf,

    /// Directory to watch for certificate rotation
    pub cert_dir: PathBuf,

    /// Path to CA bundle for server verification
    pub ca_bundle_path: PathBuf,

    /// Private key storage configuration
    pub key_storage: KeyStorageConfig,

    /// Warning window for certificate expiration (days)
    #[serde(default = "default_rotation_warning_days")]
    pub rotation_warning_days: u32,

    /// Certificate check interval in seconds
    #[serde(default = "default_cert_check_interval")]
    pub check_interval_secs: u64,
}

/// Private key storage configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum KeyStorageConfig {
    /// TPM (Trusted Platform Module) storage
    Tpm {
        /// Path to TPM device
        device_path: String,
    },
    /// HSM (Hardware Security Module) via PKCS#11
    Hsm {
        /// Path to PKCS#11 library
        pkcs11_lib: PathBuf,
        /// HSM slot ID
        slot_id: u64,
    },
    /// AWS CloudHSM
    CloudHsm {
        /// CloudHSM cluster endpoint
        endpoint: String,
        /// Key identifier
        key_id: String,
    },
}

/// Training configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrainingConfig {
    /// Number of local training epochs
    pub local_epochs: u32,

    /// FedProx proximal term coefficient (mu)
    pub fedprox_mu: f32,

    /// Checkpoint interval in seconds
    #[serde(default = "default_checkpoint_interval")]
    pub checkpoint_interval_secs: u64,

    /// Checkpoint retention period in seconds
    #[serde(default = "default_checkpoint_retention")]
    pub checkpoint_retention_secs: u64,

    /// ML framework to use
    pub framework: MlFramework,

    /// Loss tolerance for quality validation (percentage)
    #[serde(default = "default_loss_tolerance")]
    pub loss_tolerance_percent: f32,

    /// Minimum accuracy threshold (optional)
    pub min_accuracy: Option<f32>,

    /// Maximum gradient norm threshold
    #[serde(default = "default_max_gradient_norm")]
    pub max_gradient_norm: f32,
}

/// Machine learning framework
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MlFramework {
    PyTorch,
    Onnx,
    TensorFlow,
}

/// Privacy configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrivacyConfig {
    /// Enable differential privacy
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Privacy budget epsilon
    pub epsilon: f64,

    /// Privacy budget delta
    pub delta: f64,

    /// Gradient clipping threshold
    pub clip_threshold: f32,
}

/// Secure aggregation configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecureAggConfig {
    /// Enable secure aggregation
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Enable dropout recovery
    #[serde(default = "default_true")]
    pub dropout_recovery: bool,

    /// Minimum threshold for reconstruction
    #[serde(default)]
    pub threshold: Option<usize>,
}

/// Resource limits configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResourceConfig {
    /// Maximum CPU utilization (percentage)
    pub max_cpu_percent: f32,

    /// Maximum RAM usage (GB)
    pub max_ram_gb: f32,

    /// Maximum disk usage (GB)
    pub max_disk_gb: f32,

    /// Maximum GPU memory (GB, optional)
    pub max_gpu_memory_gb: Option<f32>,

    /// Warning threshold (percentage of max)
    #[serde(default = "default_warning_threshold")]
    pub warning_threshold_percent: f32,
}

/// Storage paths configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    /// Working directory for daemon
    pub working_dir: PathBuf,

    /// Model storage directory
    pub model_dir: PathBuf,

    /// Checkpoint storage directory
    pub checkpoint_dir: PathBuf,

    /// Audit log file path
    pub audit_log_path: PathBuf,

    /// Model version retention count
    #[serde(default = "default_model_retention")]
    pub model_retention_count: usize,

    /// Explainability artifacts directory (optional)
    pub explainability_dir: Option<PathBuf>,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log file path
    pub log_file: PathBuf,

    /// Enable structured JSON logging
    #[serde(default)]
    pub json_format: bool,

    /// Enable tamper-evident hash chaining
    #[serde(default)]
    pub tamper_evident: bool,

    /// Signing key for log entries (required if tamper_evident = true)
    pub signing_key: Option<String>,

    /// Enable blockchain anchoring
    #[serde(default)]
    pub blockchain_anchoring: bool,

    /// Blockchain anchoring interval (seconds)
    #[serde(default = "default_anchoring_interval")]
    pub anchoring_interval_secs: u64,
}

/// Network configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    /// Maximum concurrent requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Enable connection pooling
    #[serde(default = "default_true")]
    pub connection_pooling: bool,

    /// Pool idle timeout (seconds)
    #[serde(default = "default_pool_timeout")]
    pub pool_idle_timeout_secs: u64,

    /// Streaming upload threshold (bytes)
    #[serde(default = "default_stream_threshold")]
    pub stream_threshold_bytes: usize,
}

/// Per-model configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    /// Model identifier
    pub model_id: String,

    /// Training priority (higher = more important)
    #[serde(default = "default_priority")]
    pub priority: u8,

    /// Path to local training data
    pub data_source: PathBuf,

    /// Path to dataset schema file (optional)
    pub schema_path: Option<PathBuf>,

    /// Minimum dataset size (rows)
    #[serde(default)]
    pub min_dataset_size: Option<usize>,

    /// Maximum dataset size (rows)
    #[serde(default)]
    pub max_dataset_size: Option<usize>,

    /// Maximum data age (seconds, optional)
    pub max_data_age_secs: Option<u64>,

    /// Preprocessing script path (optional)
    pub preprocessing_script: Option<PathBuf>,
}

/// Hardware attestation configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AttestationConfig {
    /// Enable TPM attestation
    #[serde(default)]
    pub enabled: bool,

    /// Include Secure Boot status
    #[serde(default = "default_true")]
    pub include_secure_boot: bool,

    /// Submit to coordinator during auth
    #[serde(default = "default_true")]
    pub remote_attestation: bool,
}

/// Time synchronization configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeSyncConfig {
    /// Enable NTP validation
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// NTP server address
    pub ntp_server: String,

    /// Maximum allowed clock drift (seconds)
    #[serde(default = "default_max_drift")]
    pub max_drift_secs: u64,

    /// Strict mode (refuse participation if time invalid)
    #[serde(default)]
    pub strict_mode: bool,

    /// Check interval (seconds)
    #[serde(default = "default_time_check_interval")]
    pub check_interval_secs: u64,
}

// Default value functions
fn default_max_retries() -> u32 {
    5
}

fn default_rotation_warning_days() -> u32 {
    30
}

fn default_cert_check_interval() -> u64 {
    3600 // 1 hour
}

fn default_checkpoint_interval() -> u64 {
    600 // 10 minutes
}

fn default_checkpoint_retention() -> u64 {
    86400 // 24 hours
}

fn default_loss_tolerance() -> f32 {
    20.0 // 20% tolerance
}

fn default_max_gradient_norm() -> f32 {
    10.0
}

fn default_true() -> bool {
    true
}

fn default_warning_threshold() -> f32 {
    80.0 // 80% of max
}

fn default_model_retention() -> usize {
    5
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_anchoring_interval() -> u64 {
    3600 // 1 hour
}

fn default_max_concurrent() -> usize {
    10
}

fn default_pool_timeout() -> u64 {
    90
}

fn default_stream_threshold() -> usize {
    10 * 1024 * 1024 // 10 MB
}

fn default_priority() -> u8 {
    5 // Medium priority
}

fn default_max_drift() -> u64 {
    300 // 5 minutes
}

fn default_time_check_interval() -> u64 {
    3600 // 1 hour
}
