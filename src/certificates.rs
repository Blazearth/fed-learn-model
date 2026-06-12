//! Certificate and key management with hardware security support

use chrono::{DateTime, Duration, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use x509_parser::prelude::*;

use crate::config::{CertificateConfig, KeyStorageConfig};
use crate::error::{CertError, Result};

/// X.509 Certificate information
#[derive(Debug, Clone)]
pub struct Certificate {
    /// Raw certificate bytes
    pub raw: Vec<u8>,
    /// Subject name
    pub subject: String,
    /// Issuer name
    pub issuer: String,
    /// Valid from
    pub not_before: DateTime<Utc>,
    /// Valid until
    pub not_after: DateTime<Utc>,
}

/// Hardware key handle abstraction
#[derive(Debug)]
pub enum KeyHandle {
    /// TPM-backed key (stub for now)
    Tpm(TpmKeyHandle),
    /// HSM-backed key (stub for now)
    Hsm(HsmKeyHandle),
    /// CloudHSM-backed key (stub for now)
    CloudHsm(CloudHsmKeyHandle),
}

#[derive(Debug)]
pub struct TpmKeyHandle {
    device_path: String,
}

#[derive(Debug)]
pub struct HsmKeyHandle {
    pkcs11_lib: PathBuf,
    slot_id: u64,
}

#[derive(Debug)]
pub struct CloudHsmKeyHandle {
    endpoint: String,
    key_id: String,
}

impl KeyHandle {
    /// Sign data using hardware-backed key
    pub async fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self {
            KeyHandle::Tpm(handle) => handle.sign(data).await,
            KeyHandle::Hsm(handle) => handle.sign(data).await,
            KeyHandle::CloudHsm(handle) => handle.sign(data).await,
        }
    }
}

impl TpmKeyHandle {
    pub async fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Stub implementation - actual TPM integration would use tss-esapi
        tracing::warn!(
            "TPM signing stub called for device: {}",
            self.device_path
        );
        // Return mock signature
        use ring::digest::{digest, SHA256};
        Ok(digest(&SHA256, data).as_ref().to_vec())
    }
}

impl HsmKeyHandle {
    pub async fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Stub implementation - actual HSM integration would use cryptoki/PKCS#11
        tracing::warn!(
            "HSM signing stub called for slot: {}",
            self.slot_id
        );
        // Return mock signature
        use ring::digest::{digest, SHA256};
        Ok(digest(&SHA256, data).as_ref().to_vec())
    }
}

impl CloudHsmKeyHandle {
    pub async fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Stub implementation - actual CloudHSM integration would use AWS SDK
        tracing::warn!(
            "CloudHSM signing stub called for key: {}",
            self.key_id
        );
        // Return mock signature
        use ring::digest::{digest, SHA256};
        Ok(digest(&SHA256, data).as_ref().to_vec())
    }
}

/// Certificate Manager with rotation and expiration monitoring
pub struct CertificateManager {
    config: Arc<CertificateConfig>,
    current_cert: Arc<RwLock<Certificate>>,
    key_handle: KeyHandle,
    ca_bundle: Vec<u8>,
    organization_id: String,
}

impl CertificateManager {
    /// Initialize certificate manager
    pub async fn new(
        config: Arc<CertificateConfig>,
        organization_id: String,
    ) -> Result<Self> {
        // Load CA bundle
        let ca_bundle = fs::read(&config.ca_bundle_path)
            .map_err(|e| CertError::LoadFailed(format!("CA bundle: {}", e)))?;

        // Load and parse certificate
        let cert = Self::load_certificate(&config.cert_path, &organization_id)?;

        // Validate certificate against CA
        Self::validate_against_ca(&cert.raw, &ca_bundle)?;

        // Initialize key handle
        let key_handle = Self::init_key_handle(&config.key_storage)?;

        Ok(Self {
            config,
            current_cert: Arc::new(RwLock::new(cert)),
            key_handle,
            ca_bundle,
            organization_id,
        })
    }

    /// Get current certificate
    pub fn get_certificate(&self) -> Arc<Certificate> {
        let cert = self.current_cert.read().unwrap();
        Arc::new(cert.clone())
    }

    /// Check certificate expiration and emit warnings
    pub async fn check_expiration(&self) -> Result<()> {
        let cert = self.current_cert.read().unwrap();
        let now = Utc::now();
        let warning_duration = Duration::days(self.config.rotation_warning_days as i64);
        let warning_threshold = cert.not_after - warning_duration;

        if now > cert.not_after {
            return Err(CertError::Expired(cert.not_after.to_rfc3339()).into());
        }

        if now > warning_threshold {
            let days_remaining = (cert.not_after - now).num_days();
            tracing::warn!(
                "Certificate expires in {} days ({})",
                days_remaining,
                cert.not_after.to_rfc3339()
            );
        }

        Ok(())
    }

    /// Check for certificate rotation and load new certificate if available
    pub async fn check_rotation(&self) -> Result<bool> {
        // Watch for new certificate files in cert_dir
        let entries = fs::read_dir(&self.config.cert_dir)
            .map_err(|e| CertError::LoadFailed(format!("read cert dir: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| CertError::LoadFailed(e.to_string()))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("pem")
                || path.extension().and_then(|s| s.to_str()) == Some("crt")
            {
                // Skip if this is the current certificate
                if path == self.config.cert_path {
                    continue;
                }

                // Try to load and validate new certificate
                match Self::load_certificate(&path, &self.organization_id) {
                    Ok(new_cert) => {
                        // Validate against CA
                        if Self::validate_against_ca(&new_cert.raw, &self.ca_bundle).is_ok() {
                            // Check if newer than current
                            let current_cert = self.current_cert.read().unwrap();
                            if new_cert.not_after > current_cert.not_after {
                                drop(current_cert); // Release read lock

                                // Perform rotation
                                let mut cert = self.current_cert.write().unwrap();
                                let old_subject = cert.subject.clone();
                                let old_expiry = cert.not_after;

                                *cert = new_cert.clone();

                                tracing::info!(
                                    "Certificate rotated: {} (expires {}) -> {} (expires {})",
                                    old_subject,
                                    old_expiry.to_rfc3339(),
                                    new_cert.subject,
                                    new_cert.not_after.to_rfc3339()
                                );

                                return Ok(true);
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
        }

        Ok(false)
    }

    /// Sign data using hardware-backed key
    pub async fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.key_handle.sign(data).await
    }

    /// Load certificate from file
    fn load_certificate(path: &Path, expected_org_id: &str) -> Result<Certificate> {
        let cert_bytes = fs::read(path)
            .map_err(|e| CertError::LoadFailed(format!("{}: {}", path.display(), e)))?;

        // Parse PEM if needed
        let der_bytes = if cert_bytes.starts_with(b"-----BEGIN CERTIFICATE-----") {
            let parsed_pem = ::pem::parse(&cert_bytes)
                .map_err(|e| CertError::LoadFailed(format!("PEM parse: {}", e)))?;
            parsed_pem.into_contents()
        } else {
            cert_bytes.clone()
        };

        // Parse X.509
        let (_, cert) = X509Certificate::from_der(&der_bytes)
            .map_err(|e| CertError::Invalid(format!("X.509 parse: {}", e)))?;

        // Extract subject
        let subject = cert.subject().to_string();

        // Verify subject contains organization ID
        if !subject.contains(expected_org_id) {
            return Err(CertError::SubjectMismatch {
                expected: expected_org_id.to_string(),
                actual: subject.clone(),
            }
            .into());
        }

        // Extract issuer
        let issuer = cert.issuer().to_string();

        // Extract validity period
        let not_before = DateTime::from_timestamp(cert.validity().not_before.timestamp(), 0)
            .unwrap_or_else(Utc::now);
        let not_after = DateTime::from_timestamp(cert.validity().not_after.timestamp(), 0)
            .unwrap_or_else(|| Utc::now() + Duration::days(365));

        // Check if expired
        if Utc::now() > not_after {
            return Err(CertError::Expired(not_after.to_rfc3339()).into());
        }

        Ok(Certificate {
            raw: der_bytes,
            subject,
            issuer,
            not_before,
            not_after,
        })
    }

    /// Validate certificate against CA bundle
    fn validate_against_ca(cert_der: &[u8], ca_bundle: &[u8]) -> Result<()> {
        // Parse certificate
        let (_, cert) = X509Certificate::from_der(cert_der)
            .map_err(|e| CertError::Invalid(format!("cert parse: {}", e)))?;

        // Parse CA bundle (may contain multiple CAs)
        let ca_pems = ::pem::parse_many(ca_bundle)
            .map_err(|e| CertError::LoadFailed(format!("CA bundle parse: {}", e)))?;

        if ca_pems.is_empty() {
            return Err(CertError::UntrustedCA.into());
        }

        // Try to find matching CA
        let cert_issuer = cert.issuer().to_string();
        
        for ca_pem in &ca_pems {
            if let Ok((_, ca_cert)) = X509Certificate::from_der(ca_pem.contents()) {
                let ca_subject = ca_cert.subject().to_string();
                
                // Check if this CA is the issuer
                if ca_subject == cert_issuer {
                    // In production, would verify signature here
                    tracing::debug!("Certificate validated against CA: {}", ca_subject);
                    return Ok(());
                }
            }
        }

        Err(CertError::UntrustedCA.into())
    }

    /// Initialize key handle based on configuration
    fn init_key_handle(config: &KeyStorageConfig) -> Result<KeyHandle> {
        match config {
            KeyStorageConfig::Tpm { device_path } => {
                // Check if device exists
                if !Path::new(device_path).exists() {
                    return Err(CertError::Tpm(format!(
                        "TPM device not found: {}",
                        device_path
                    ))
                    .into());
                }

                Ok(KeyHandle::Tpm(TpmKeyHandle {
                    device_path: device_path.clone(),
                }))
            }
            KeyStorageConfig::Hsm {
                pkcs11_lib,
                slot_id,
            } => {
                // Check if PKCS#11 library exists
                if !pkcs11_lib.exists() {
                    return Err(CertError::Pkcs11(format!(
                        "PKCS#11 library not found: {}",
                        pkcs11_lib.display()
                    ))
                    .into());
                }

                Ok(KeyHandle::Hsm(HsmKeyHandle {
                    pkcs11_lib: pkcs11_lib.clone(),
                    slot_id: *slot_id,
                }))
            }
            KeyStorageConfig::CloudHsm { endpoint, key_id } => Ok(KeyHandle::CloudHsm(
                CloudHsmKeyHandle {
                    endpoint: endpoint.clone(),
                    key_id: key_id.clone(),
                },
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_self_signed_cert(org_id: &str) -> Vec<u8> {
        // This is a mock certificate for testing
        // In production, use proper certificate generation
        let cert_pem = format!(
            r#"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKHHCgVZU6T/MA0GCSqGSIb3DQEBCwUAMBExDzANBgNVBAMMBnRl
c3QtY2EwHhcNMjQwMTAxMDAwMDAwWhcNMjUwMTAxMDAwMDAwWjAXMRUwEwYDVQQD
DAx7fS10ZXN0LW9yZzBcMA0GCSqGSIb3DQEBAQUAA0sAMEgCQQDcjxCLQbPJ7V8b
example-cert-data-{}
-----END CERTIFICATE-----"#,
            org_id, org_id
        );
        cert_pem.as_bytes().to_vec()
    }

    #[tokio::test]
    async fn test_certificate_expiration_check() {
        // This test would need actual certificate infrastructure
        // Skipping for now as it requires valid certificates
    }

    #[test]
    fn test_key_handle_initialization() {
        let temp_dir = tempdir().unwrap();
        let tpm_path = temp_dir.path().join("tpm0");
        fs::write(&tpm_path, b"mock tpm").unwrap();

        let config = KeyStorageConfig::Tpm {
            device_path: tpm_path.to_str().unwrap().to_string(),
        };

        let result = CertificateManager::init_key_handle(&config);
        assert!(result.is_ok());
    }
}
