use anyhow::{bail, Context};
use indicatif::ProgressBar;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use fl_client_daemon::config::Configuration;

/// Slim epoch response — only the fields the CLI actually uses.
/// Avoids the Vec<u8> mismatch with the coordinator's base64 strings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EpochInfo {
    pub epoch_number:   u64,
    pub model_id:       String,
    pub model_version:  String,
    pub model_hash:     String,
    pub status:         Option<String>,
    pub fedprox_mu:     Option<f64>,
    pub privacy_epsilon: Option<f64>,
    pub privacy_delta:   Option<f64>,
    pub secure_agg_threshold: Option<u64>,
    pub secure_agg_participants: Option<Vec<Value>>,
    pub architecture_hash: Option<String>,
}

pub struct CoordinatorClient {
    client:   Client,
    base_url: String,
    model_id: String,
}

impl CoordinatorClient {
    pub fn new(cfg: &Configuration) -> anyhow::Result<Self> {
        // Load cert: prefer COORDINATOR_CERT_B64 env var, else cert_path file
        let cert_pem = load_bytes_b64_or_file("COORDINATOR_CERT_B64", &cfg.certificates.cert_path)?;

        // Load key: prefer COORDINATOR_KEY_B64 env var, else derive key path from cert_path
        // (cert = org-aiims.pem → key = org-aiims.key in same dir)
        let key_path = cfg.certificates.cert_path.with_extension("key");
        let key_pem  = load_bytes_b64_or_file("COORDINATOR_KEY_B64", &key_path)?;

        // reqwest Identity requires PEM cert + PEM key concatenated
        let mut combined = cert_pem.clone();
        combined.extend_from_slice(&key_pem);
        let identity = reqwest::Identity::from_pem(&combined)
            .context("failed to build mTLS identity — check cert+key paths in config")?;

        let client = Client::builder()
            .use_rustls_tls()
            .identity(identity)
            .timeout(Duration::from_secs(cfg.coordinator.request_timeout_secs))
            .build()?;

        let model_id = cfg.models.first()
            .map(|m| m.model_id.clone())
            .unwrap_or_else(|| "fraud-detection-v2".to_string());

        Ok(Self { client, base_url: cfg.coordinator.base_url.clone(), model_id })
    }

    pub async fn get_active_epoch(&self) -> anyhow::Result<Option<EpochInfo>> {
        let url = format!("{}/api/epochs/active?model_id={}", self.base_url, self.model_id);
        let res = self.client.get(&url).send().await?;
        if res.status().as_u16() == 404 { return Ok(None); }
        if !res.status().is_success() {
            bail!("coordinator returned {}: {}", res.status(), res.text().await.unwrap_or_default());
        }
        Ok(Some(res.json::<EpochInfo>().await?))
    }

    pub async fn get_model_download_url(&self, version: &str) -> anyhow::Result<String> {
        let res = self.client
            .post(format!("{}/api/models/download-url", self.base_url))
            .json(&serde_json::json!({ "model_id": self.model_id, "model_version": version }))
            .send().await?;
        if !res.status().is_success() {
            bail!("get_model_download_url: {}", res.status());
        }
        let v: Value = res.json().await?;
        v["download_url"].as_str().map(String::from)
            .ok_or_else(|| anyhow::anyhow!("missing download_url in response"))
    }

    pub async fn get_update_upload_url(&self, epoch: u64) -> anyhow::Result<String> {
        let res = self.client
            .post(format!("{}/api/updates/upload-url", self.base_url))
            .json(&serde_json::json!({ "model_id": self.model_id, "epoch_number": epoch }))
            .send().await?;
        if !res.status().is_success() {
            bail!("get_update_upload_url: {}", res.status());
        }
        let v: Value = res.json().await?;
        v["upload_url"].as_str().map(String::from)
            .ok_or_else(|| anyhow::anyhow!("missing upload_url in response"))
    }

    pub async fn submit_complete(&self, epoch: u64, hash: &str) -> anyhow::Result<()> {
        let res = self.client
            .post(format!("{}/api/updates/complete", self.base_url))
            .json(&serde_json::json!({
                "model_id": self.model_id,
                "epoch_number": epoch,
                "update_hash": hash,
            }))
            .send().await?;
        if !res.status().is_success() {
            bail!("submit_complete: {} — {}", res.status(), res.text().await.unwrap_or_default());
        }
        Ok(())
    }

    /// Download bytes from a pre-signed URL, updating progress bar as chunks arrive.
    pub async fn download_bytes(&self, url: &str, pb: &ProgressBar) -> anyhow::Result<Vec<u8>> {
        use futures_util::StreamExt;
        let res = self.client.get(url).send().await?;
        if !res.status().is_success() {
            bail!("S3 download failed: {}", res.status());
        }
        let mut stream = res.bytes_stream();
        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            pb.inc(chunk.len() as u64);
            buf.extend_from_slice(&chunk);
        }
        pb.finish_with_message("✓ Done");
        Ok(buf)
    }

    /// Upload bytes to a pre-signed S3 PUT URL with progress bar.
    pub async fn upload_bytes(&self, url: &str, data: Vec<u8>, pb: &ProgressBar) -> anyhow::Result<()> {
        let total = data.len() as u64;
        let res = self.client.put(url).body(data).send().await?;
        pb.inc(total);
        pb.finish_with_message("✓ Done");
        if !res.status().is_success() {
            bail!("S3 upload failed: {}", res.status());
        }
        Ok(())
    }
}

fn load_bytes_b64_or_file(env_var: &str, fallback: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    if let Ok(b64) = std::env::var(env_var) {
        use base64::Engine;
        return base64::engine::general_purpose::STANDARD.decode(b64)
            .context(format!("invalid base64 in {env_var}"));
    }
    std::fs::read(fallback).with_context(|| format!("cannot read {}", fallback.display()))
}
