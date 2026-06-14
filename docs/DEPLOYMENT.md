# Deployment Guide

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| OS | Ubuntu 22.04 / RHEL 9 | Ubuntu 24.04 LTS |
| Architecture | x86_64 | x86_64 |
| CPU | 4 cores | 8+ cores |
| RAM | 4 GB | 16 GB |
| Disk | 20 GB | 100 GB SSD |
| Network | 10 Mbps outbound | 100 Mbps |
| TPM | Optional | TPM 2.0 (tpm2-abrmd) |

## Installation Steps

### 1. Build the binary

```bash
# Install Rust if not present
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Clone the repository
git clone https://github.com/your-org/fl-client-daemon.git
cd fl-client-daemon

# Build release binary
cargo build --release
```

### 2. Run the installer

```bash
sudo ./config/install.sh
```

The installer creates:
- `/usr/local/bin/fl-client-daemon` — daemon binary
- `/etc/fl-daemon/` — configuration directory (750, root:fl-daemon)
- `/etc/fl-daemon/certs/` — certificate directory
- `/var/lib/fl-daemon/` — working directory (models, checkpoints, audit log)
- `/var/log/fl-daemon/` — log directory
- `fl-daemon` system user and group

### 3. Install certificates

Place the following files in `/etc/fl-daemon/certs/`:

```
/etc/fl-daemon/certs/
  client.pem          # Organization X.509 certificate (PEM)
  ca-bundle.pem       # Coordinator CA trust bundle (PEM)
```

Set permissions:
```bash
sudo chmod 640 /etc/fl-daemon/certs/*.pem
sudo chown root:fl-daemon /etc/fl-daemon/certs/*.pem
```

### 4. Configure the daemon

```bash
sudo nano /etc/fl-daemon/config.toml
```

Required fields:
```toml
organization_id = "your-org-id"          # matches cert subject

[coordinator]
base_url = "https://coordinator.example.com"

[certificates]
cert_path = "/etc/fl-daemon/certs/client.pem"
ca_bundle_path = "/etc/fl-daemon/certs/ca-bundle.pem"

[[models]]
model_id = "your-model-id"
data_source = "/data/training/dataset.parquet"
```

See `config/config.example.toml` for the full reference.

### 5. Configure TPM (recommended)

```bash
# Install TPM software stack
sudo apt install tpm2-abrmd tpm2-tools

# Verify TPM is accessible
tpm2_getcap properties-fixed | grep TPMFamilyIndicator

# Add fl-daemon to tss group
sudo usermod -aG tss fl-daemon
```

Then set in config:
```toml
[certificates.key_storage]
type = "Tpm"
device_path = "/dev/tpmrm0"
```

### 6. Enable and start the service

```bash
sudo systemctl daemon-reload
sudo systemctl enable rust-client-daemon
sudo systemctl start rust-client-daemon
```

### 7. Verify startup

```bash
sudo systemctl status rust-client-daemon
sudo journalctl -u rust-client-daemon -n 50 --no-pager
```

Expected output on healthy start:
```
INFO fl_client_daemon: Federated Learning Client Daemon starting version=0.1.0 org_id=...
INFO fl_client_daemon: Configuration loaded successfully
INFO fl_client_daemon: Polling coordinator for epoch metadata
```

## Configuration Guide

### Resource limits

Set hard limits to prevent the daemon from monopolizing system resources:

```toml
[resources]
max_cpu_percent = 80.0    # throttle if CPU exceeds this
max_ram_gb = 8.0          # pause training if RAM exceeds this
max_disk_gb = 100.0       # stop writes if disk exceeds this
warning_threshold_percent = 80.0
```

### Training checkpoints

Checkpoints allow training to resume after a crash or preemption:

```toml
[training]
checkpoint_interval_secs = 600     # save every 10 minutes
checkpoint_retention_secs = 86400  # keep for 24 hours
```

On restart the daemon automatically detects and resumes from the latest checkpoint.

### Multi-model configuration

Add a `[[models]]` section for each model the organization participates in:

```toml
[[models]]
model_id = "fraud-detection-v2"
priority = 1
data_source = "/data/fraud/training.parquet"
schema_path = "/etc/fl-daemon/fraud_detection.schema.json"

[[models]]
model_id = "credit-scoring-v1"
priority = 2
data_source = "/data/credit/training.parquet"
```

Higher-priority jobs (lower number) preempt lower-priority jobs when resources are constrained.

## Monitoring and Logging

### Structured JSON logs

```toml
[logging]
json_format = true
level = "info"
log_file = "/var/log/fl-daemon/daemon.log"
```

Key log fields: `timestamp`, `level`, `epoch`, `org_id`, `event_type`, `message`.

### Log rotation (logrotate)

Create `/etc/logrotate.d/fl-daemon`:
```
/var/log/fl-daemon/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    postrotate
        systemctl kill -s HUP rust-client-daemon
    endscript
}
```

### Key events to monitor

| Event | Log field | Severity |
|-------|-----------|----------|
| Training round started | `training_round_started` | INFO |
| Upload complete | `update_uploaded` | INFO |
| Certificate expiry warning | `cert_expiry_warning` | WARN |
| Resource limit exceeded | `resource_limit_exceeded` | WARN |
| Model signature invalid | `model_signature_invalid` | ERROR |
| Attestation rejected | `attestation_rejected` | CRITICAL |
| Log tampering detected | `log_tampering_detected` | CRITICAL |

### Prometheus metrics

When `metrics_endpoint` is configured, the daemon exposes metrics at `http://localhost:9090/metrics`:
- `fl_training_round_duration_seconds`
- `fl_upload_bytes_total`
- `fl_resource_cpu_percent`
- `fl_resource_ram_gb`

## Configuration Reload

Reload configuration without restarting (Requirement 15):

```bash
sudo systemctl reload rust-client-daemon
# or
sudo kill -HUP $(systemctl show --property=MainPID rust-client-daemon | cut -d= -f2)
```

Configuration changes that take effect immediately:
- `coordinator.poll_interval_secs`
- `resources.*`
- `logging.level`

Changes requiring restart:
- `organization_id`
- `certificates.*`
- `secure_aggregation.enabled`

## Graceful Shutdown

```bash
sudo systemctl stop rust-client-daemon
```

The daemon:
1. Receives SIGTERM
2. Completes any in-progress S3 upload
3. Saves training checkpoint to disk
4. Closes all network connections
5. Terminates within 60 seconds (configurable `TimeoutStopSec`)

## Troubleshooting

### Daemon fails to start: "cannot read own binary"

The daemon needs read access to its own binary for attestation:
```bash
sudo chmod 755 /usr/local/bin/fl-client-daemon
```

### Authentication failure: "certificate expired"

```bash
# Check expiration
openssl x509 -noout -dates -in /etc/fl-daemon/certs/client.pem
# Rotate: place new cert in /etc/fl-daemon/certs/ and reload
sudo systemctl reload rust-client-daemon
```

### Training not starting: "dataset validation failed"

Check the log for the specific validation error:
```bash
journalctl -u rust-client-daemon | grep "dataset"
```

Common causes: schema mismatch, row count below minimum, NULL values in required columns.

### High memory usage

Lower `training.local_epochs` or reduce `resources.max_ram_gb` to trigger throttling sooner.

### Clock drift warning

```bash
sudo apt install chrony
sudo systemctl enable --now chrony
chronyc tracking
```

For strict mode, set `time_sync.strict_validation = true` in config.
