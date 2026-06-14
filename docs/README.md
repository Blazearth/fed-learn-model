# Federated Learning Client Daemon

A secure, production-grade Rust service that enables organizations to participate in collaborative machine learning without sharing raw data.

## Overview

The FL Client Daemon runs within each organization's secure boundary. It handles local model training, applies Differential Privacy, masks updates via Secure Aggregation, and communicates with an AWS-hosted cloud coordinator using outbound-only mTLS connections. Raw data never leaves the organization.

```
Organization Boundary
  ├─ Training Data (local CSV / Parquet)
  └─ FL Client Daemon
       ├─ Config Manager
       ├─ Certificate Manager (TPM / HSM)
       ├─ Model Manager
       ├─ Training Engine (FedProx)
       ├─ Privacy Engine (Differential Privacy)
       ├─ SecureAgg Engine
       ├─ Audit Engine (tamper-evident logs)
       ├─ Metrics Engine
       ├─ Checkpoint Manager
       └─ Network Engine (mTLS, S3)
              │ outbound only
              ▼
        AWS Cloud Coordinator + S3 + Hyperledger Fabric
```

## Features

- **FedProx training** — handles non-IID data distributions across organizations
- **Differential Privacy** — gradient clipping + calibrated Gaussian noise (ε/δ budget)
- **Secure Aggregation** — pairwise ECDH masking; coordinator never sees plaintext updates
- **Byzantine resilience** — server-side robust aggregation filters poisoned updates
- **Hardware key protection** — private keys stored in TPM 2.0 / HSM / AWS CloudHSM; never extracted to application memory
- **Hardware attestation** — TPM-based proof of daemon integrity submitted at startup
- **Memory zeroization** — gradient buffers, key material, and masks are zeroed after use
- **Certificate rotation** — hot reload of new certificates without restart
- **Training checkpoints** — resume mid-round after crashes or preemption
- **Multi-model scheduling** — priority-based scheduler with preemption
- **Data drift detection** — PSI-based feature drift monitoring
- **Tamper-evident audit log** — SHA-256 hash chain + optional Hyperledger Fabric anchoring
- **Supply chain security** — binary hash verification + SBOM generation

## Requirements

| Component | Minimum |
|-----------|---------|
| OS | Linux (x86_64), systemd |
| Rust | 1.75+ |
| TPM | TPM 2.0 (optional, recommended) |
| RAM | 4 GB (8 GB recommended) |
| Disk | 20 GB working space |

## Quick Start

### 1. Build

```bash
cargo build --release
```

### 2. Install

```bash
sudo ./config/install.sh
```

### 3. Configure

```bash
sudo cp /etc/fl-daemon/config.toml /etc/fl-daemon/config.toml.bak
sudo nano /etc/fl-daemon/config.toml
```

Minimum required fields:
- `organization_id` — must match your X.509 certificate subject
- `coordinator.base_url` — cloud coordinator URL
- `certificates.cert_path` — path to your org certificate
- `certificates.ca_bundle_path` — coordinator CA bundle
- `models[*].data_source` — path to local training data

### 4. Start the service

```bash
sudo systemctl enable rust-client-daemon
sudo systemctl start rust-client-daemon
sudo journalctl -u rust-client-daemon -f
```

## Configuration

Full configuration reference: [`config/config.example.toml`](../config/config.example.toml)

Key sections:

| Section | Purpose |
|---------|---------|
| `[coordinator]` | Cloud coordinator URL, polling interval, retry settings |
| `[certificates]` | Certificate paths, key storage backend, rotation window |
| `[training]` | FedProx mu, local epochs, quality validation thresholds |
| `[privacy]` | Differential privacy ε/δ budget, clipping threshold |
| `[secure_aggregation]` | Masking, dropout recovery, threshold |
| `[resources]` | CPU/RAM/disk/GPU hard limits |
| `[storage]` | Working directories, model retention count |
| `[logging]` | Log level, JSON format, tamper-evident hash chain |
| `[[models]]` | Per-model ID, priority, data source, schema |

## Testing

```bash
# Run all tests (unit + property-based + integration)
cargo test

# Run with verbose output
cargo test -- --nocapture

# Run a specific module
cargo test privacy
cargo test model
cargo test secureagg
```

The test suite includes 43 correctness properties validated across 100–200 randomized iterations each.

## Architecture

See [`Enterprise_Federated_AI_Platform.md`](../Enterprise_Federated_AI_Platform.md) for the full system design and [`design.md`](../.kiro/specs/rust-client-daemon/design.md) for the daemon module design.

## Security

See [`SECURITY.md`](SECURITY.md) for the security architecture, hardware requirements, and threat model.

## Deployment

See [`DEPLOYMENT.md`](DEPLOYMENT.md) for production deployment, monitoring, and troubleshooting.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for development setup, coding conventions, and the pull request process.
