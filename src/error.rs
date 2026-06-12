//! Error types for the federated learning client daemon

use thiserror::Error;

/// Main result type for daemon operations
pub type Result<T> = std::result::Result<T, DaemonError>;

/// Top-level error type for all daemon operations
#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Certificate error: {0}")]
    Certificate(#[from] CertError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("Model error: {0}")]
    Model(#[from] ModelError),

    #[error("Training error: {0}")]
    Training(#[from] TrainingError),

    #[error("Privacy error: {0}")]
    Privacy(#[from] PrivacyError),

    #[error("Secure aggregation error: {0}")]
    SecureAgg(#[from] SecureAggError),

    #[error("Audit error: {0}")]
    Audit(#[from] AuditError),

    #[error("Checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),

    #[error("Metrics error: {0}")]
    Metrics(#[from] MetricsError),

    #[error("Data error: {0}")]
    Data(#[from] DataError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Configuration-related errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    FileNotFound(String),

    #[error("Invalid configuration syntax at {location}: {message}")]
    InvalidSyntax { location: String, message: String },

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid value for field {field}: {reason}")]
    InvalidValue { field: String, reason: String },

    #[error("Certificate path does not exist or is not readable: {0}")]
    CertPathInvalid(String),

    #[error("Failed to parse TOML: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Certificate and key management errors
#[derive(Debug, Error)]
pub enum CertError {
    #[error("Certificate expired on {0}")]
    Expired(String),

    #[error("Invalid certificate: {0}")]
    Invalid(String),

    #[error("Certificate not issued by trusted CA")]
    UntrustedCA,

    #[error("Certificate subject mismatch: expected {expected}, got {actual}")]
    SubjectMismatch { expected: String, actual: String },

    #[error("Hardware key storage error: {0}")]
    HardwareKey(String),

    #[error("Unencrypted private key file detected: {0}")]
    UnencryptedKey(String),

    #[error("Failed to load certificate: {0}")]
    LoadFailed(String),

    #[error("TPM error: {0}")]
    Tpm(String),

    #[error("HSM error: {0}")]
    Hsm(String),

    #[error("PKCS#11 error: {0}")]
    Pkcs11(String),
}

/// Network communication errors
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),

    #[error("Connection timeout after {0}s")]
    Timeout(u64),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Permanent failure: {0}")]
    PermanentFailure(String),

    #[error("Retryable error: {0}")]
    RetryableError(String),

    #[error("Max retries exceeded")]
    MaxRetriesExceeded,

    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
}

/// Model management errors
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("Model download failed: {0}")]
    DownloadFailed(String),

    #[error("Model hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Invalid model signature")]
    InvalidSignature,

    #[error("Model signature verification failed: {0}")]
    SignatureVerificationFailed(String),

    #[error("Incompatible model architecture: {0}")]
    IncompatibleArchitecture(String),

    #[error("Incompatible framework version: {0}")]
    IncompatibleFramework(String),

    #[error("Model format not supported: {0}")]
    UnsupportedFormat(String),

    #[error("Model rollback failed: {0}")]
    RollbackFailed(String),

    #[error("No previous model version available for rollback")]
    NoPreviousVersion,
}

/// Training-related errors
#[derive(Debug, Error)]
pub enum TrainingError {
    #[error("Training failed: {0}")]
    Failed(String),

    #[error("Dataset loading failed: {0}")]
    DatasetLoadFailed(String),

    #[error("Preprocessing failed: {0}")]
    PreprocessingFailed(String),

    #[error("Model quality validation failed: {0}")]
    QualityValidationFailed(String),

    #[error("NaN detected in gradients")]
    NaNInGradients,

    #[error("Exploding gradients: norm {norm} exceeds threshold {threshold}")]
    ExplodingGradients { norm: f32, threshold: f32 },

    #[error("Loss outside tolerance: local={local}, global={global}, tolerance={tolerance}%")]
    LossOutsideTolerance {
        local: f32,
        global: f32,
        tolerance: f32,
    },
}

/// Data validation and processing errors
#[derive(Debug, Error)]
pub enum DataError {
    #[error("Corrupted data file: {0}")]
    CorruptedFile(String),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("Invalid feature count: expected {expected}, got {actual}")]
    InvalidFeatureCount { expected: usize, actual: usize },

    #[error("NULL values found in required field: {0}")]
    NullInRequiredField(String),

    #[error("Dataset too old: age {age} exceeds maximum {max}")]
    DatasetTooOld { age: String, max: String },

    #[error("Dataset size {size} outside bounds [{min}, {max}]")]
    SizeOutOfBounds { size: usize, min: usize, max: usize },

    #[error("Class imbalance exceeds threshold: {0}")]
    ClassImbalance(String),
}

/// Privacy engine errors
#[derive(Debug, Error)]
pub enum PrivacyError {
    #[error("Invalid privacy budget: epsilon={epsilon}, delta={delta}")]
    InvalidBudget { epsilon: f64, delta: f64 },

    #[error("Privacy budget exhausted")]
    BudgetExhausted,

    #[error("Gradient clipping failed: {0}")]
    ClippingFailed(String),

    #[error("Noise generation failed: {0}")]
    NoiseGenerationFailed(String),
}

/// Secure aggregation errors
#[derive(Debug, Error)]
pub enum SecureAggError {
    #[error("Key generation failed: {0}")]
    KeyGenerationFailed(String),

    #[error("Mask generation failed: {0}")]
    MaskGenerationFailed(String),

    #[error("Dropout recovery failed: {0}")]
    DropoutRecoveryFailed(String),

    #[error("Threshold not met: needed {needed}, got {actual}")]
    ThresholdNotMet { needed: usize, actual: usize },

    #[error("Participant not found: {0}")]
    ParticipantNotFound(String),
}

/// Audit logging errors
#[derive(Debug, Error)]
pub enum AuditError {
    #[error("Failed to write audit log: {0}")]
    WriteFailed(String),

    #[error("Log integrity verification failed: {0}")]
    IntegrityCheckFailed(String),

    #[error("Log tampering detected at entry {0}")]
    TamperingDetected(usize),

    #[error("Failed to sign log entry: {0}")]
    SigningFailed(String),

    #[error("Blockchain anchoring failed: {0}")]
    BlockchainAnchoringFailed(String),
}

/// Checkpoint management errors
#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("Failed to save checkpoint: {0}")]
    SaveFailed(String),

    #[error("Failed to load checkpoint: {0}")]
    LoadFailed(String),

    #[error("No checkpoint found for job {0}")]
    NotFound(String),

    #[error("Checkpoint corrupted: {0}")]
    Corrupted(String),
}

/// Metrics and monitoring errors
#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("Resource measurement failed: {0}")]
    MeasurementFailed(String),

    #[error("Resource limit exceeded: {resource} at {current} > {limit}")]
    LimitExceeded {
        resource: String,
        current: f32,
        limit: f32,
    },

    #[error("Drift computation failed: {0}")]
    DriftComputationFailed(String),

    #[error("Explainability generation failed: {0}")]
    ExplainabilityFailed(String),
}

// Conversion implementations for common error types
impl From<serde_json::Error> for DaemonError {
    fn from(err: serde_json::Error) -> Self {
        DaemonError::Serialization(err.to_string())
    }
}

impl From<toml::ser::Error> for DaemonError {
    fn from(err: toml::ser::Error) -> Self {
        DaemonError::Serialization(err.to_string())
    }
}
