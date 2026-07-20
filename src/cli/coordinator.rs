use anyhow::{bail, Context};
use indicatif::ProgressBar;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use fl_client_daemon::config::Configuration;
use fl_client_daemon::types::EpochMetadata;

pub struct CoordinatorClient {
    client:   Client,
    base_url: String,
    model_id: String,
}

impl CoordinatorClient {
    pub fn new(cfg: &Configuration) -> anyhow::Result<Self> {
        // Load cert + key: prefer base64 env vars (Vercel/cloud), fall back to file paths
        let cert_pem = load_bytes_b64_or_file("COORDINATOR_CERT_B64", &cfg.certificates.cert_path)?;
        let key_pem  = load_bytes_b64_or_file("COORDINATOR_KEY_B64",  &cfg.certificates.cert_path
            // ponytail: key path lives separately — read from cert sibling .key file
            // The daemon config has cert_path but key is alongside it with .key extension
            )?;

        let identity = reqwest::Identity::from_pem(&{
            let mut combined = cert_pem.clone();
            combined.extend_from_slice(&key_pem);
            combined
        })
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

    pub async fn get_active_epoch(&self) -> anyhow::Result<Option<EpochMetadata>> {
        let url = format!("{}/api/epochs/active?model_id={}", self.base_url, self.model_id);
        let res = self.client.get(&url).send().await?;
        if res.status().as_u16() == 404 { return Ok(None); }
        if !res.status().is_success() {
            bail!("coordinator returned {}: {}", res.status(), res.text().await.unwrap_or_default());
        }
        Ok(Some(res.json::<EpochMetadata>().await?))
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
