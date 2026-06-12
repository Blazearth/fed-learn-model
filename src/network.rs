//! Network communication and HTTP client with mTLS and retry logic

use reqwest::{Client, ClientBuilder};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

use crate::certificates::Certificate;
use crate::error::{NetworkError, Result};
use crate::types::EpochMetadata;

/// Backoff state for exponential backoff
#[derive(Debug, Clone)]
struct BackoffState {
    max_delay_secs: u64,
    attempt: u32,
}

impl BackoffState {
    fn new(max_delay_secs: u64) -> Self {
        Self {
            max_delay_secs,
            attempt: 0,
        }
    }

    fn next_delay(&mut self) -> u64 {
        // delay = min(initial * 2^attempt, max) where initial is always 1
        let delay = std::cmp::min(
            2_u64.pow(self.attempt),
            self.max_delay_secs,
        );
        self.attempt += 1;
        delay
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Network engine for HTTP communication with coordinator
pub struct NetworkEngine {
    client: Client,
    base_url: String,
    backoff_state: Arc<Mutex<BackoffState>>,
    max_retries: u32,
}

impl NetworkEngine {
    /// Create new network engine with mTLS
    pub fn new(
        base_url: String,
        _cert: Arc<Certificate>,
        _ca_bundle: Vec<u8>,
        request_timeout_secs: u64,
        max_backoff_secs: u64,
        max_retries: u32,
    ) -> Result<Self> {
        // Build HTTP client with rustls
        // Note: Full mTLS configuration would require rustls setup with certificates
        // For now, we use a simplified version
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(request_timeout_secs))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            backoff_state: Arc::new(Mutex::new(BackoffState::new(max_backoff_secs))),
            max_retries,
        })
    }

    /// Poll for active epoch metadata
    pub async fn poll_epoch_metadata(&self, model_id: &str) -> Result<Option<EpochMetadata>> {
        let url = format!("{}/api/epochs/active?model_id={}", self.base_url, model_id);

        self.request_with_retry(|| async {
            let response = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            if response.status() == 404 {
                return Ok(None);
            }

            if !response.status().is_success() {
                return Err(NetworkError::InvalidResponse(format!(
                    "status: {}",
                    response.status()
                ))
                .into());
            }

            let metadata: EpochMetadata = response
                .json()
                .await
                .map_err(|e| NetworkError::InvalidResponse(e.to_string()))?;

            Ok(Some(metadata))
        })
        .await
    }

    /// Request pre-signed download URL for model
    pub async fn request_download_url(
        &self,
        model_id: &str,
        model_version: &str,
    ) -> Result<String> {
        let url = format!("{}/api/models/download-url", self.base_url);

        self.request_with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .json(&serde_json::json!({
                    "model_id": model_id,
                    "model_version": model_version,
                }))
                .send()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            if !response.status().is_success() {
                return Err(NetworkError::InvalidResponse(format!(
                    "status: {}",
                    response.status()
                ))
                .into());
            }

            let result: serde_json::Value = response
                .json()
                .await
                .map_err(|e| NetworkError::InvalidResponse(e.to_string()))?;

            let download_url = result["download_url"]
                .as_str()
                .ok_or_else(|| NetworkError::InvalidResponse("missing download_url".to_string()))?;

            Ok(download_url.to_string())
        })
        .await
    }

    /// Download model from S3 using pre-signed URL
    pub async fn download_from_s3(&self, presigned_url: &str) -> Result<Vec<u8>> {
        self.request_with_retry(|| async {
            let response = self
                .client
                .get(presigned_url)
                .send()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            if !response.status().is_success() {
                return Err(NetworkError::InvalidResponse(format!(
                    "S3 download failed: {}",
                    response.status()
                ))
                .into());
            }

            let bytes = response
                .bytes()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            Ok(bytes.to_vec())
        })
        .await
    }

    /// Request pre-signed upload URL for protected update
    pub async fn request_upload_url(&self, model_id: &str, epoch_number: u64) -> Result<String> {
        let url = format!("{}/api/updates/upload-url", self.base_url);

        self.request_with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .json(&serde_json::json!({
                    "model_id": model_id,
                    "epoch_number": epoch_number,
                }))
                .send()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            if !response.status().is_success() {
                return Err(NetworkError::InvalidResponse(format!(
                    "status: {}",
                    response.status()
                ))
                .into());
            }

            let result: serde_json::Value = response
                .json()
                .await
                .map_err(|e| NetworkError::InvalidResponse(e.to_string()))?;

            let upload_url = result["upload_url"]
                .as_str()
                .ok_or_else(|| NetworkError::InvalidResponse("missing upload_url".to_string()))?;

            Ok(upload_url.to_string())
        })
        .await
    }

    /// Upload protected update to S3
    pub async fn upload_to_s3(&self, presigned_url: &str, data: Vec<u8>) -> Result<String> {
        // Compute SHA-256 hash
        use ring::digest::{digest, SHA256};
        let hash = digest(&SHA256, &data);
        let hash_hex: String = hash.as_ref().iter().map(|b| format!("{:02x}", b)).collect();

        self.request_with_retry(|| async {
            let data_clone = data.clone();
            let response = self
                .client
                .put(presigned_url)
                .body(data_clone)
                .send()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            if !response.status().is_success() {
                return Err(NetworkError::InvalidResponse(format!(
                    "S3 upload failed: {}",
                    response.status()
                ))
                .into());
            }

            Ok(hash_hex.clone())
        })
        .await
    }

    /// Submit completion notification
    pub async fn submit_completion(
        &self,
        model_id: &str,
        epoch_number: u64,
        update_hash: &str,
    ) -> Result<()> {
        let url = format!("{}/api/updates/complete", self.base_url);

        self.request_with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .json(&serde_json::json!({
                    "model_id": model_id,
                    "epoch_number": epoch_number,
                    "update_hash": update_hash,
                }))
                .send()
                .await
                .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

            if !response.status().is_success() {
                return Err(NetworkError::InvalidResponse(format!(
                    "status: {}",
                    response.status()
                ))
                .into());
            }

            Ok(())
        })
        .await
    }

    /// Execute request with exponential backoff retry
    async fn request_with_retry<F, Fut, T>(&self, mut request_fn: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempts = 0;

        loop {
            match request_fn().await {
                Ok(result) => {
                    // Reset backoff on success
                    self.backoff_state.lock().unwrap().reset();
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;

                    if attempts >= self.max_retries {
                        return Err(NetworkError::MaxRetriesExceeded.into());
                    }

                    // Check if error is retryable
                    if !Self::is_retryable(&e) {
                        return Err(e);
                    }

                    // Calculate backoff delay
                    let delay = self.backoff_state.lock().unwrap().next_delay();

                    tracing::warn!(
                        "Request failed (attempt {}/{}), retrying in {}s: {:?}",
                        attempts,
                        self.max_retries,
                        delay,
                        e
                    );

                    sleep(Duration::from_secs(delay)).await;
                }
            }
        }
    }

    /// Check if error is retryable
    fn is_retryable(error: &crate::error::DaemonError) -> bool {
        matches!(
            error,
            crate::error::DaemonError::Network(NetworkError::RequestFailed(_))
                | crate::error::DaemonError::Network(NetworkError::Timeout(_))
                | crate::error::DaemonError::Network(NetworkError::RetryableError(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_state() {
        let mut backoff = BackoffState::new(300);

        assert_eq!(backoff.next_delay(), 1);
        assert_eq!(backoff.next_delay(), 2);
        assert_eq!(backoff.next_delay(), 4);
        assert_eq!(backoff.next_delay(), 8);

        backoff.reset();
        assert_eq!(backoff.next_delay(), 1);
    }

    #[test]
    fn test_backoff_max_delay() {
        let mut backoff = BackoffState::new(10);

        for _ in 0..10 {
            let delay = backoff.next_delay();
            assert!(delay <= 10, "Delay {} exceeds max 10", delay);
        }
    }

    // ---------------------------------------------------------------
    // Property-based tests
    // Feature: rust-client-daemon
    // Property 5: Exponential Backoff Bounds
    // Validates: Requirements 3.3, 3.4, 4.7, 8.5
    // For any sequence of polling or network failures, the backoff delay SHALL
    // increase exponentially and SHALL never exceed the configured maximum.
    // ---------------------------------------------------------------
    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(100))]

        #[test]
        fn prop_backoff_never_exceeds_max(
            max_secs in 1u64..=3600,
            num_calls in 1usize..=30
        ) {
            let mut backoff = BackoffState::new(max_secs);
            for _ in 0..num_calls {
                let delay = backoff.next_delay();
                proptest::prop_assert!(
                    delay <= max_secs,
                    "delay {} exceeded max {}", delay, max_secs
                );
            }
        }

        #[test]
        fn prop_backoff_resets_correctly(max_secs in 1u64..=3600) {
            let mut backoff = BackoffState::new(max_secs);
            // Advance several steps
            for _ in 0..5 {
                backoff.next_delay();
            }
            // After reset, first delay must be 1 (2^0)
            backoff.reset();
            let first = backoff.next_delay();
            proptest::prop_assert_eq!(first, 1u64);
        }

        #[test]
        fn prop_backoff_is_non_decreasing(
            max_secs in 16u64..=3600,
            num_calls in 2usize..=15
        ) {
            let mut backoff = BackoffState::new(max_secs);
            let mut prev = backoff.next_delay();
            for _ in 1..num_calls {
                let next = backoff.next_delay();
                proptest::prop_assert!(
                    next >= prev,
                    "delay went backwards: {} -> {}", prev, next
                );
                prev = next;
            }
        }
    }
}
