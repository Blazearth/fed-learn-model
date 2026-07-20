# Deployment Guide

This guide covers deploying the `fl-client-daemon` and `fl-client` binaries on a production Linux server inside an organization's network boundary.

For coordinator (AWS) deployment see [`coordinator/AWS_DEPLOY.md`](../coordinator/AWS_DEPLOY.md).

---

## System Requirements

| Component | Minimum | Recommended |
|---|---|---|
| OS | Ubuntu 22.04 / RHEL 9 | Ubuntu 24.04 LTS |
| Architecture | x86_64 | x86_64 |
| CPU | 4 cores | 8+ cores |
| RAM | 4 GB | 16 GB |
| Disk | 20 GB | 100 GB SSD |
| Network | 10 Mbps outbound | 100 Mbps |
| TPM | optional | TPM 2.0 (tpm2-abrmd) |

---

## Installation

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup update stable
```

### 2. Clone and build

```bash
git clone https://github.com/Blazearth/fed-learn-model.git
cd fed-learn-model/federated_learning_model

# Build both binaries
cargo build --release

# Verify
./target/release/fl-client --version
./target/release/fl-client-daemon --help
```

### 3. Run the system installer

```bash
sudo ./config/install.sh
```

The installer creates:

| Path | Purpose |
|---|---|
| `/usr/local/bin/fl-client-daemon` | daemon binary |
| `/usr/local/bin/fl-client` | CLI binary |
| `/etc/fl-daemon/` | config directory (root:fl-daemon, 750) |
| `/etc/fl-daemon/certs/` | certificate directory |
| `/var/lib/fl-daemon/` | working directory (models, checkpoints) |
| `/var/log/fl-daemon/` | log directory |
| `fl-daemon` system user + group | service account |

### 4. Install certificates

The federation operator provides two files per organization. Place them in `/etc/fl-daemon/certs/`:

```
/etc/fl-daemon/certs/
  org-<your-id>.pem      ← Organization X.509 certificate
  org-<your-id>.key      ← Private key (keep this secret)
  ca-bundle.pem          ← Coordinator CA trust bundle
```

Set permissions:
```bash
sudo chmod 640 /etc/fl-daemon/certs/*.pem /etc/fl-daemon/certs/*.key
sudo chown root:fl-daemon /etc/fl-daemon/certs/*
```

> **Key path convention** — the CLI and daemon derive the key path from `cert_path` by replacing the extension with `.key`. If your cert is `org-aiims.pem`, the key must be `org-aiims.key` in the same directory.

### 5. Configure

Use the interactive wizard for first-time setup:

```bash
fl-client init
```

Or manually edit `/etc/fl-daemon/config.toml`. Minimum required fields:

```toml
organization_id = "org-aiims"   # must match your certificate subject CN

[coordinator]
base_url             = "https://coordinator.fed-learn.online"
poll_interval_secs   = 10
max_backoff_secs     = 300
request_timeout_secs = 30

[certificates]
cert_path      = "/etc/fl-daemon/certs/org-aiims.pem"
cert_dir       = "/etc/fl-daemon/certs"
ca_bundle_path = "/etc/fl-daemon/certs/ca-bundle.pem"

[certificates.key_storage]
type        = "tpm"
device_path = "/dev/tpmrm0"

[training]
local_epochs = 5
fedprox_mu   = 0.01
framework    = "pytorch"

[privacy]
epsilon        = 1.0
delta          = 1e-5
clip_threshold = 1.0

[secure_aggregation]

[resources]
max_cpu_percent = 80.0
max_ram_gb      = 8.0
max_disk_gb     = 100.0

[storage]
working_dir    = "/var/lib/fl-daemon/work"
model_dir      = "/var/lib/fl-daemon/models"
checkpoint_dir = "/var/lib/fl-daemon/checkpoints"
audit_log_path = "/var/log/fl-daemon/audit.log"

[logging]
level    = "info"
log_file = "/var/log/fl-daemon/daemon.log"

[network]

[[models]]
model_id    = "fraud-detection-v2"
data_source = "/data/training/records.parquet"
```

Full reference: [`config/config.example.toml`](../config/config.example.toml)

### 6. Configure TPM (recommended)

```bash
sudo apt install tpm2-abrmd tpm2-tools
sudo usermod -aG tss fl-daemon
tpm2_getcap properties-fixed | grep TPMFamilyIndicator   # verify accessible
```

Set in config:
```toml
[certificates.key_storage]
type        = "tpm"
device_path = "/dev/tpmrm0"
```

### 7. Enable and start the daemon

```bash
sudo systemctl daemon-reload
sudo systemctl enable rust-client-daemon
sudo systemctl start rust-client-daemon
```

### 8. Verify

```bash
sudo systemctl status rust-client-daemon
sudo journalctl -u rust-client-daemon -n 50 --no-pager
```

Expected healthy output:
```
INFO fl_client_daemon: Federated Learning Client Daemon starting version=0.1.0 org_id=org-aiims
INFO fl_client_daemon: Configuration loaded successfully
INFO fl_client_daemon: Polling coordinator for epoch metadata
```

---

## Configuration Reference

### Training

```toml
[training]
local_epochs             = 5      # local gradient steps per round
fedprox_mu               = 0.01   # proximal term — higher = tighter to global model
framework                = "pytorch"
checkpoint_interval_secs = 600    # save checkpoint every 10 minutes
checkpoint_retention_secs = 86400 # keep checkpoints for 24 hours
loss_tolerance_percent   = 20.0   # reject update if loss diverges more than this
max_gradient_norm        = 10.0   # quality gate before submission
```

Checkpoints allow the daemon to resume mid-round after a crash or preemption. On restart the daemon automatically detects and loads the latest checkpoint.

### Privacy

```toml
[privacy]
enabled        = true
epsilon        = 1.0    # privacy budget — smaller = stronger privacy
delta          = 1e-5
clip_threshold = 1.0    # L2 gradient clipping bound (sensitivity C)
```

### Resources

```toml
[resources]
max_cpu_percent           = 80.0   # throttle training above this
max_ram_gb                = 8.0    # pause training above this
max_disk_gb               = 100.0  # stop writes above this
warning_threshold_percent = 80.0   # warn at this fraction of max
```

### Multi-model

Each `[[models]]` section configures participation in one model's federation:

```toml
[[models]]
model_id    = "fraud-detection-v2"
priority    = 1                              # lower = higher priority
data_source = "/data/fraud/records.parquet"
schema_path = "/etc/fl-daemon/fraud_detection.schema.json"

[[models]]
model_id    = "credit-scoring-v1"
priority    = 2
data_source = "/data/credit/records.parquet"
```

Higher-priority jobs (lower number) preempt lower-priority jobs when resources are constrained.

---

## Operations

### Configuration reload (no restart)

```bash
sudo systemctl reload rust-client-daemon
# or:
sudo kill -HUP $(systemctl show --property=MainPID rust-client-daemon | cut -d= -f2)
```

Hot-reloadable settings: `coordinator.poll_interval_secs`, `resources.*`, `logging.level`

Requires restart: `organization_id`, `certificates.*`, `secure_aggregation.enabled`

### Graceful shutdown

```bash
sudo systemctl stop rust-client-daemon
```

The daemon:
1. Receives SIGTERM
2. Completes any in-progress S3 upload
3. Saves training checkpoint to disk
4. Closes all network connections
5. Exits within 60 seconds (configurable `TimeoutStopSec`)

### Manual pipeline (using fl-client)

For operators who prefer manual control over the automated daemon:

```bash
fl-client epoch      # check if a round is open
fl-client download   # download latest global model
fl-client train      # run local training + apply privacy
fl-client submit     # upload and notify coordinator
# or all in one:
fl-client run
```

---

## Monitoring

### Structured JSON logs

```toml
[logging]
json_format = true
level       = "info"
log_file    = "/var/log/fl-daemon/daemon.log"
```

Key log fields: `timestamp`, `level`, `epoch`, `org_id`, `event_type`, `message`

### Key events to alert on

| Event | Severity | Action |
|---|---|---|
| `training_round_started` | INFO | Normal |
| `update_uploaded` | INFO | Normal |
| `cert_expiry_warning` | WARN | Rotate certificate within 30 days |
| `resource_limit_exceeded` | WARN | Scale up hardware or reduce `local_epochs` |
| `model_signature_invalid` | ERROR | Investigate coordinator key rotation |
| `attestation_rejected` | CRITICAL | Binary may be tampered — investigate immediately |
| `log_tampering_detected` | CRITICAL | Audit log chain broken — investigate immediately |

### Prometheus metrics

The daemon exposes metrics at `http://localhost:9090/metrics` when `metrics_endpoint` is set:

```
fl_training_round_duration_seconds
fl_upload_bytes_total
fl_resource_cpu_percent
fl_resource_ram_gb
```

### Log rotation

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

---

## Troubleshooting

### Daemon fails to start: "cannot read own binary"

Binary attestation requires read access to the binary itself:
```bash
sudo chmod 755 /usr/local/bin/fl-client-daemon
```

### Authentication failure: "certificate expired"

```bash
# Check expiry date
openssl x509 -noout -dates -in /etc/fl-daemon/certs/org-aiims.pem

# Rotate: place new cert+key, then reload (no restart needed)
sudo cp new-org-aiims.pem /etc/fl-daemon/certs/org-aiims.pem
sudo cp new-org-aiims.key /etc/fl-daemon/certs/org-aiims.key
sudo systemctl reload rust-client-daemon
```

### mTLS identity build failure

The key file must be in the same directory as the cert, with `.key` extension:
```bash
ls /etc/fl-daemon/certs/
# should show: org-aiims.pem  org-aiims.key  ca-bundle.pem
```

### Training not starting: "dataset validation failed"

```bash
journalctl -u rust-client-daemon | grep "dataset"
```

Common causes:
- Schema mismatch — feature count or names don't match `schema_path`
- Row count below `min_dataset_size`
- NULL values in required (non-nullable) columns
- File path doesn't exist — verify `data_source` path is accessible by `fl-daemon` user

### "No active epoch for model X"

The coordinator has no active training round. Check the Sangrah dashboard at `sangrah.vercel.app` or contact the federation operator. The daemon will keep polling and start automatically when an epoch is activated.

### High memory usage

Reduce `training.local_epochs` or lower `resources.max_ram_gb` to trigger throttling sooner. Check for memory leaks with:
```bash
journalctl -u rust-client-daemon | grep "resource_limit"
```

### Clock drift warning

```bash
sudo apt install chrony
sudo systemctl enable --now chrony
chronyc tracking
```

For strict enforcement (daemon refuses participation if clock drift too large):
```toml
[time_sync]
enabled    = true
strict_mode = true
max_drift_secs = 300
```

### Coordinator returns 403 Forbidden

Your certificate may not be registered or may have been revoked. Contact the federation operator with your `organization_id` and cert fingerprint:

```bash
openssl x509 -noout -fingerprint -in /etc/fl-daemon/certs/org-aiims.pem
```

### Coordinator notification succeeds but state not saved

If `fl-client submit` prints "submission recorded by coordinator but local state write failed", the coordinator has your update. Run `fl-client epoch` to check if the epoch advanced — if it did, the submission succeeded. The state file can be manually corrected:

```bash
echo '{"last_submitted_epoch": N, "submitted_at": "2026-07-21T00:00:00Z"}' \
  > /var/lib/fl-daemon/work/submission_state.json
```
