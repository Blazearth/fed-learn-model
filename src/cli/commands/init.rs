use std::path::PathBuf;
use std::process::ExitCode;

use dialoguer::{Confirm, Input};

use crate::output;

pub async fn run() -> ExitCode {
    println!("fl-client init — interactive configuration wizard\n");

    let org_id: String  = prompt("Organization ID (e.g. org-aiims)");
    let coord_url: String = prompt_default("Coordinator URL", "https://coordinator.fed-learn.online");
    let cert_path: String = prompt("Path to org certificate (.pem)");
    let ca_path: String   = prompt("Path to CA bundle (.pem)");
    let model_id: String  = prompt_default("Model ID", "fraud-detection-v2");
    let data_src: String  = prompt("Path to local training data (.parquet or .csv)");

    // Validate cert exists
    if !PathBuf::from(&cert_path).exists() {
        output::error(&format!("Cert file not found: {cert_path}"));
        return ExitCode::FAILURE;
    }

    // Determine output path
    let out_path = dirs::home_dir()
        .map(|h| h.join(".fl-client/config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"));

    if out_path.exists() {
        let ok = Confirm::new()
            .with_prompt(format!("{} already exists. Overwrite?", out_path.display()))
            .default(false)
            .interact()
            .unwrap_or(false);
        if !ok {
            println!("Cancelled — config unchanged.");
            return ExitCode::SUCCESS;
        }
    }

    // Write minimal config (ponytail: omitting optional fields keeps it readable)
    let toml = format!(
        r#"organization_id = "{org_id}"

[coordinator]
base_url = "{coord_url}"
poll_interval_secs = 10
max_backoff_secs = 300
request_timeout_secs = 30

[certificates]
cert_path = "{cert_path}"
cert_dir  = "{cert_path}"
ca_bundle_path = "{ca_path}"

[certificates.key_storage]
type = "tpm"
device_path = "/dev/tpmrm0"

[training]
local_epochs = 5
fedprox_mu = 0.01
framework = "pytorch"

[privacy]
epsilon = 1.0
delta = 1e-5
clip_threshold = 1.0

[secure_aggregation]

[resources]
max_cpu_percent = 80.0
max_ram_gb = 8.0
max_disk_gb = 100.0

[storage]
working_dir = "/var/lib/fl-daemon/work"
model_dir   = "/var/lib/fl-daemon/models"
checkpoint_dir = "/var/lib/fl-daemon/checkpoints"
audit_log_path = "/var/log/fl-daemon/audit.log"

[logging]
level = "info"
log_file = "/var/log/fl-daemon/daemon.log"

[network]

[[models]]
model_id    = "{model_id}"
data_source = "{data_src}"
"#
    );

    if let Some(parent) = out_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            output::error(&format!("Cannot create config directory: {e}"));
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = std::fs::write(&out_path, toml) {
        output::error(&format!("Failed to write config: {e}"));
        return ExitCode::FAILURE;
    }

    output::success(&format!("✓ Config written to {}", out_path.display()));
    ExitCode::SUCCESS
}

fn prompt(label: &str) -> String {
    Input::<String>::new().with_prompt(label).interact_text().unwrap_or_default()
}

fn prompt_default(label: &str, default: &str) -> String {
    Input::<String>::new()
        .with_prompt(label)
        .default(default.to_string())
        .interact_text()
        .unwrap_or_else(|_| default.to_string())
}
