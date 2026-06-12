//! Configuration manager with validation and hot reload

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::config::Configuration;
use crate::error::{ConfigError, Result};

/// Configuration manager with hot reload support
#[derive(Debug)]
pub struct ConfigManager {
    /// Current configuration (thread-safe)
    config: Arc<RwLock<Configuration>>,
    /// Path to configuration file
    file_path: PathBuf,
}

impl ConfigManager {
    /// Load configuration from file path
    pub async fn new(file_path: PathBuf) -> Result<Self> {
        // Check if file exists
        if !file_path.exists() {
            return Err(ConfigError::FileNotFound(
                file_path.display().to_string(),
            )
            .into());
        }

        // Read and parse configuration
        let config = Self::load_and_validate(&file_path)?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            file_path,
        })
    }

    /// Get current configuration snapshot
    pub fn get(&self) -> Arc<Configuration> {
        let config = self.config.read().unwrap();
        Arc::new(config.clone())
    }

    /// Reload configuration from disk
    pub async fn reload(&self) -> Result<()> {
        // Load and validate new configuration
        let new_config = Self::load_and_validate(&self.file_path)?;

        // Atomically swap configuration
        let mut config = self.config.write().unwrap();
        *config = new_config;

        tracing::info!(
            "Configuration reloaded successfully from {}",
            self.file_path.display()
        );

        Ok(())
    }

    /// Load configuration from file and validate
    fn load_and_validate(path: &Path) -> Result<Configuration> {
        // Read file contents
        let contents = fs::read_to_string(path).map_err(|e| ConfigError::Io(e))?;

        // Parse TOML
        let config: Configuration = toml::from_str(&contents).map_err(|e| {
            ConfigError::InvalidSyntax {
                location: "config file".to_string(),
                message: e.to_string(),
            }
        })?;

        // Validate configuration
        Self::validate(&config)?;

        Ok(config)
    }

    /// Validate configuration structure and values
    fn validate(config: &Configuration) -> Result<()> {
        // Validate organization ID is not empty
        if config.organization_id.is_empty() {
            return Err(ConfigError::MissingField("organization_id".to_string()).into());
        }

        // Validate coordinator config
        Self::validate_coordinator(&config.coordinator)?;

        // Validate certificate config
        Self::validate_certificates(&config.certificates)?;

        // Validate training config
        Self::validate_training(&config.training)?;

        // Validate privacy config
        Self::validate_privacy(&config.privacy)?;

        // Validate resource config
        Self::validate_resources(&config.resources)?;

        // Validate storage config
        Self::validate_storage(&config.storage)?;

        // Validate network config
        Self::validate_network(&config.network)?;

        // Validate models
        if config.models.is_empty() {
            return Err(ConfigError::InvalidValue {
                field: "models".to_string(),
                reason: "at least one model must be configured".to_string(),
            }
            .into());
        }

        for model in &config.models {
            Self::validate_model(model)?;
        }

        Ok(())
    }

    fn validate_coordinator(config: &crate::config::CoordinatorConfig) -> Result<()> {
        if config.base_url.is_empty() {
            return Err(ConfigError::MissingField("coordinator.base_url".to_string()).into());
        }

        if config.poll_interval_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "coordinator.poll_interval_secs".to_string(),
                reason: "must be greater than 0".to_string(),
            }
            .into());
        }

        if config.max_backoff_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "coordinator.max_backoff_secs".to_string(),
                reason: "must be greater than 0".to_string(),
            }
            .into());
        }

        if config.request_timeout_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "coordinator.request_timeout_secs".to_string(),
                reason: "must be greater than 0".to_string(),
            }
            .into());
        }

        Ok(())
    }

    fn validate_certificates(config: &crate::config::CertificateConfig) -> Result<()> {
        // Validate certificate path exists
        if !config.cert_path.exists() {
            return Err(ConfigError::CertPathInvalid(
                config.cert_path.display().to_string(),
            )
            .into());
        }

        // Validate certificate path is readable
        if fs::metadata(&config.cert_path).is_err() {
            return Err(ConfigError::CertPathInvalid(format!(
                "{} is not readable",
                config.cert_path.display()
            ))
            .into());
        }

        // Validate cert directory exists
        if !config.cert_dir.exists() {
            return Err(ConfigError::InvalidValue {
                field: "certificates.cert_dir".to_string(),
                reason: format!("directory {} does not exist", config.cert_dir.display()),
            }
            .into());
        }

        // Validate CA bundle path exists
        if !config.ca_bundle_path.exists() {
            return Err(ConfigError::CertPathInvalid(
                config.ca_bundle_path.display().to_string(),
            )
            .into());
        }

        Ok(())
    }

    fn validate_training(config: &crate::config::TrainingConfig) -> Result<()> {
        if config.local_epochs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "training.local_epochs".to_string(),
                reason: "must be greater than 0".to_string(),
            }
            .into());
        }

        if config.fedprox_mu < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "training.fedprox_mu".to_string(),
                reason: "must be non-negative".to_string(),
            }
            .into());
        }

        if config.loss_tolerance_percent < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "training.loss_tolerance_percent".to_string(),
                reason: "must be non-negative".to_string(),
            }
            .into());
        }

        if config.max_gradient_norm <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "training.max_gradient_norm".to_string(),
                reason: "must be positive".to_string(),
            }
            .into());
        }

        Ok(())
    }

    fn validate_privacy(config: &crate::config::PrivacyConfig) -> Result<()> {
        if config.enabled {
            if config.epsilon <= 0.0 {
                return Err(ConfigError::InvalidValue {
                    field: "privacy.epsilon".to_string(),
                    reason: "must be positive".to_string(),
                }
                .into());
            }

            if config.delta <= 0.0 || config.delta >= 1.0 {
                return Err(ConfigError::InvalidValue {
                    field: "privacy.delta".to_string(),
                    reason: "must be between 0 and 1".to_string(),
                }
                .into());
            }

            if config.clip_threshold <= 0.0 {
                return Err(ConfigError::InvalidValue {
                    field: "privacy.clip_threshold".to_string(),
                    reason: "must be positive".to_string(),
                }
                .into());
            }
        }

        Ok(())
    }

    fn validate_resources(config: &crate::config::ResourceConfig) -> Result<()> {
        if config.max_cpu_percent <= 0.0 || config.max_cpu_percent > 100.0 {
            return Err(ConfigError::InvalidValue {
                field: "resources.max_cpu_percent".to_string(),
                reason: "must be between 0 and 100".to_string(),
            }
            .into());
        }

        if config.max_ram_gb <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "resources.max_ram_gb".to_string(),
                reason: "must be positive".to_string(),
            }
            .into());
        }

        if config.max_disk_gb <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "resources.max_disk_gb".to_string(),
                reason: "must be positive".to_string(),
            }
            .into());
        }

        if let Some(gpu_mem) = config.max_gpu_memory_gb {
            if gpu_mem <= 0.0 {
                return Err(ConfigError::InvalidValue {
                    field: "resources.max_gpu_memory_gb".to_string(),
                    reason: "must be positive".to_string(),
                }
                .into());
            }
        }

        Ok(())
    }

    fn validate_storage(config: &crate::config::StorageConfig) -> Result<()> {
        if config.model_retention_count == 0 {
            return Err(ConfigError::InvalidValue {
                field: "storage.model_retention_count".to_string(),
                reason: "must be greater than 0".to_string(),
            }
            .into());
        }

        Ok(())
    }

    fn validate_network(config: &crate::config::NetworkConfig) -> Result<()> {
        if config.max_concurrent_requests == 0 {
            return Err(ConfigError::InvalidValue {
                field: "network.max_concurrent_requests".to_string(),
                reason: "must be greater than 0".to_string(),
            }
            .into());
        }

        Ok(())
    }

    fn validate_model(config: &crate::config::ModelConfig) -> Result<()> {
        if config.model_id.is_empty() {
            return Err(ConfigError::InvalidValue {
                field: "model.model_id".to_string(),
                reason: "must not be empty".to_string(),
            }
            .into());
        }

        if !config.data_source.exists() {
            return Err(ConfigError::InvalidValue {
                field: format!("model.{}.data_source", config.model_id),
                reason: format!(
                    "data source {} does not exist",
                    config.data_source.display()
                ),
            }
            .into());
        }

        if let Some(min) = config.min_dataset_size {
            if min == 0 {
                return Err(ConfigError::InvalidValue {
                    field: format!("model.{}.min_dataset_size", config.model_id),
                    reason: "must be greater than 0 if specified".to_string(),
                }
                .into());
            }
        }

        if let (Some(min), Some(max)) = (config.min_dataset_size, config.max_dataset_size) {
            if min > max {
                return Err(ConfigError::InvalidValue {
                    field: format!("model.{}.dataset_size", config.model_id),
                    reason: "min_dataset_size must be less than or equal to max_dataset_size"
                        .to_string(),
                }
                .into());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_config() -> String {
        r#"
organization_id = "test-org"

[coordinator]
base_url = "https://coordinator.example.com"
poll_interval_secs = 60
max_backoff_secs = 300
request_timeout_secs = 30

[certificates]
cert_path = "/tmp/cert.pem"
cert_dir = "/tmp/certs"
ca_bundle_path = "/tmp/ca.pem"
check_interval_secs = 3600

[certificates.key_storage]
type = "tpm"
device_path = "/dev/tpm0"

[training]
local_epochs = 5
fedprox_mu = 0.1
framework = "pytorch"

[privacy]
enabled = true
epsilon = 1.0
delta = 0.00001
clip_threshold = 1.0

[secure_aggregation]
enabled = true

[resources]
max_cpu_percent = 80.0
max_ram_gb = 16.0
max_disk_gb = 100.0

[storage]
working_dir = "/var/lib/fl-daemon"
model_dir = "/var/lib/fl-daemon/models"
checkpoint_dir = "/var/lib/fl-daemon/checkpoints"
audit_log_path = "/var/log/fl-daemon/audit.log"

[logging]
level = "info"
log_file = "/var/log/fl-daemon/daemon.log"

[network]
max_concurrent_requests = 10

[[models]]
model_id = "fraud-detection"
priority = 8
data_source = "/data/fraud"
"#
        .to_string()
    }

    #[tokio::test]
    async fn test_load_valid_configuration() {
        // Create temporary certificate files
        let cert_file = NamedTempFile::new().unwrap();
        let ca_file = NamedTempFile::new().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let cert_dir = tempfile::tempdir().unwrap();

        let mut config_str = create_test_config();
        config_str = config_str.replace("/tmp/cert.pem", cert_file.path().to_str().unwrap());
        config_str = config_str.replace("/tmp/ca.pem", ca_file.path().to_str().unwrap());
        config_str = config_str.replace("/tmp/certs", cert_dir.path().to_str().unwrap());
        config_str = config_str.replace("/data/fraud", data_dir.path().to_str().unwrap());

        let mut config_file = NamedTempFile::new().unwrap();
        config_file.write_all(config_str.as_bytes()).unwrap();
        config_file.flush().unwrap();

        let result = ConfigManager::new(config_file.path().to_path_buf()).await;
        assert!(result.is_ok(), "Failed to load valid configuration: {:?}", result.err());

        let manager = result.unwrap();
        let config = manager.get();
        assert_eq!(config.organization_id, "test-org");
        assert_eq!(config.coordinator.poll_interval_secs, 60);
    }

    #[tokio::test]
    async fn test_missing_config_file() {
        let result = ConfigManager::new(PathBuf::from("/nonexistent/config.toml")).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::error::DaemonError::Config(ConfigError::FileNotFound(_))
        ));
    }

    #[test]
    fn test_validate_invalid_epsilon() {
        let mut config: Configuration = toml::from_str(&create_test_config()).unwrap();
        config.privacy.epsilon = -1.0;

        let result = ConfigManager::validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_cpu_percent() {
        let mut config: Configuration = toml::from_str(&create_test_config()).unwrap();
        config.resources.max_cpu_percent = 150.0;

        let result = ConfigManager::validate(&config);
        assert!(result.is_err());
    }
}
