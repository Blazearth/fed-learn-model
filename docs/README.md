# Sangrah — Federated Learning Platform

> *Unite Your Intelligence. Keep Your Data.*

Sangrah is a production-grade federated learning platform for regulated industries — hospitals, banks, insurance firms, and government bodies — that need to collaborate on AI models without sharing raw data.

**The model moves to the data. Not the other way around.**

---

## What's in this repo

| Component | Language | Purpose |
|---|---|---|
| `fl-client-daemon` | Rust | Background service — runs inside each org, handles training + privacy + upload |
| `fl-client` | Rust | CLI tool — human-facing interface for operators and data scientists |
| `coordinator/` | Python + AWS SAM | Cloud coordinator — epoch orchestration, aggregation, audit |

Live coordinator API: **`https://coordinator.fed-learn.online`**  
Dashboard UI: **`https://sangrah.vercel.app`**  
Source (this repo): **`https://github.com/Blazearth/fed-learn-model`**

---

## How it works

```
┌──────────────────────────────┐     ┌──────────────────────────────┐
│   Organization A (AIIMS)     │     │   Organization B (KGMU)      │
│                              │     │                              │
│  Training Data (stays local) │     │  Training Data (stays local) │
│         │                    │     │         │                    │
│  ┌──────▼──────────────┐     │     │  ┌──────▼──────────────┐     │
│  │  fl-client-daemon   │     │     │  │  fl-client-daemon   │     │
│  │  ① FedProx train    │     │     │  │  ① FedProx train    │     │
│  │  ② Diff. Privacy    │     │     │  │  ② Diff. Privacy    │     │
│  │  ③ Secure Agg mask  │     │     │  │  ③ Secure Agg mask  │     │
│  └──────┬──────────────┘     │     │  └──────┬──────────────┘     │
└─────────┼────────────────────┘     └─────────┼────────────────────┘
          │  outbound mTLS only                 │  outbound mTLS only
          └──────────────┬──────────────────────┘
                         │
          ┌──────────────▼──────────────────────┐
          │        AWS Cloud Coordinator         │
          │  API Gateway (mTLS) · Lambda · S3   │
          │  DynamoDB · ECS Fargate · CloudWatch │
          └──────────────────────────────────────┘
                         │
          ┌──────────────▼──────────────────────┐
          │          Sangrah UI (observer)       │
          │   fetches live data from AWS only    │
          └──────────────────────────────────────┘
```

Raw data **never** leaves the organization boundary. Only masked, DP-noised gradient updates cross the wire.

---

## Quick start (for org operators)

The fastest path to join a training round:

```bash
# 1. Build both binaries
git clone https://github.com/Blazearth/fed-learn-model.git
cd fed-learn-model/federated_learning_model
cargo build --release

# 2. Run the setup wizard
./target/release/fl-client init

# 3. Run the full pipeline — epoch → download → train → submit
./target/release/fl-client run
```

Or use the interactive menu:

```bash
./target/release/fl-client
```

```
══════════════════════════════════════════
 Federated Learning Client
 org-aiims · https://coordinator.fed-learn.online
══════════════════════════════════════════
  1. View Active Epoch
  2. Download Model
  3. Train Model
  4. Submit Update
  5. Run Full Pipeline
  6. View Status
  0. Exit
```

---

## System requirements

| Component | Minimum | Recommended |
|---|---|---|
| OS | Linux x86_64, systemd | Ubuntu 24.04 LTS |
| Rust | 1.75+ | latest stable |
| RAM | 4 GB | 16 GB |
| Disk | 20 GB | 100 GB SSD |
| Network | 10 Mbps outbound | 100 Mbps |
| TPM | optional | TPM 2.0 (tpm2-abrmd) |

---

## Privacy and security stack

Every model update passes through five layers before leaving your network:

| Layer | What it does |
|---|---|
| **mTLS** | Every request authenticated with your X.509 org certificate — rejected at API Gateway without a valid cert |
| **FedProx training** | Local training with proximal term $\mu$ — prevents drift on non-IID enterprise data |
| **Differential Privacy** | Gradient clipping + Gaussian noise calibrated to $(\varepsilon, \delta)$ budget — protects against reconstruction attacks |
| **Secure Aggregation** | ECDH pairwise masks that cancel in the aggregate — coordinator sees only the sum, never individual updates |
| **Byzantine resilience** | Server-side Multi-Krum scoring rejects poisoned updates from malicious participants |

**DP noise formula:**
$$\sigma = \frac{\sqrt{2\ln(1.25/\delta)}}{\varepsilon}$$

With defaults ($\varepsilon = 1.0$, $\delta = 10^{-5}$), $\sigma \approx 4.75$ — a meaningful privacy guarantee without destroying model quality.

---

## Binaries

### `fl-client-daemon` — the background service

Runs as a systemd service inside your network boundary. Handles the full federated learning lifecycle automatically: polling for epochs, downloading models, training, applying privacy, and submitting updates.

```bash
# Build
cargo build --release --bin fl-client-daemon

# Install as systemd service
sudo ./config/install.sh
sudo systemctl enable rust-client-daemon
sudo systemctl start rust-client-daemon
sudo journalctl -u rust-client-daemon -f
```

See [DEPLOYMENT.md](DEPLOYMENT.md) for full setup instructions.

### `fl-client` — the CLI

Human-facing tool for operators and data scientists. Wraps the same training pipeline in simple subcommands with progress bars, colored output, and an interactive menu.

```bash
cargo build --release --bin fl-client

fl-client whoami      # check identity
fl-client epoch       # query active round
fl-client download    # get latest global model
fl-client train       # run local training + privacy
fl-client submit      # upload update + notify coordinator
fl-client run         # all of the above in one command
fl-client init        # first-time setup wizard
```

See [FL_CLIENT_CLI.md](FL_CLIENT_CLI.md) for full CLI documentation.

---

## Configuration

Both binaries share the same `config.toml`. The CLI searches:
1. `--config <path>` flag
2. `/etc/fl-daemon/config.toml`
3. `~/.fl-client/config.toml`

Minimal example:

```toml
organization_id = "org-aiims"

[coordinator]
base_url             = "https://coordinator.fed-learn.online"
poll_interval_secs   = 10
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

---

## Testing

```bash
# All tests — unit + property-based + integration
cargo test

# Specific module
cargo test privacy
cargo test secureagg
cargo test training

# With output
cargo test -- --nocapture

# More property iterations
PROPTEST_CASES=500 cargo test
```

The test suite has **171 tests** including 43 correctness properties validated across 100–200 randomized iterations each covering:

- DP gradient clipping always enforces max norm
- DP noise scale is always positive for valid $(\varepsilon, \delta)$
- Secure aggregation masks always cancel in the aggregate
- FedProx proximal term applied for all valid $\mu$ values
- Exponential backoff never exceeds configured maximum
- Model update gradients are always finite
- Dataset size bounds correctly reject out-of-range inputs

---

## Project structure

```
federated_learning_model/
├── src/
│   ├── main.rs              # fl-client-daemon entry point
│   ├── lib.rs               # module declarations
│   ├── config.rs            # configuration structs
│   ├── config/manager.rs    # load, validate, hot reload
│   ├── types.rs             # shared data structures
│   ├── error.rs             # error type hierarchy
│   ├── audit.rs             # tamper-evident hash-chain log
│   ├── attestation.rs       # TPM hardware attestation
│   ├── certificates.rs      # cert management, TPM/HSM
│   ├── checkpoint.rs        # training checkpoint save/resume
│   ├── memory.rs            # mlock + zeroization helpers
│   ├── metrics.rs           # resource monitoring, drift detection
│   ├── model.rs             # model download, verification, rollback
│   ├── network.rs           # HTTP client, mTLS, retry
│   ├── privacy.rs           # differential privacy engine
│   ├── scheduler.rs         # multi-model priority scheduler
│   ├── secureagg.rs         # secure aggregation, dropout recovery
│   ├── supply_chain.rs      # binary hash verification, SBOM
│   ├── time_sync.rs         # NTP drift validation
│   ├── training.rs          # FedProx training engine
│   └── cli/                 # fl-client binary
│       ├── main.rs          # CLI entry point
│       ├── args.rs          # clap argument parser
│       ├── coordinator.rs   # mTLS coordinator client
│       ├── config_loader.rs # config path resolution
│       ├── menu.rs          # interactive menu
│       ├── output.rs        # colored terminal output
│       ├── progress.rs      # indicatif progress bars
│       ├── state.rs         # submission state (atomic write)
│       └── commands/        # one file per subcommand
│           ├── whoami.rs
│           ├── epoch.rs
│           ├── download.rs
│           ├── train.rs
│           ├── submit.rs
│           ├── run.rs
│           ├── init.rs
│           ├── status.rs
│           └── version.rs
├── coordinator/             # AWS coordinator (Python + SAM)
├── config/
│   ├── config.example.toml
│   ├── install.sh
│   ├── rust-client-daemon.service
│   ├── fraud_detection.schema.json
│   └── credit_scoring.schema.json
├── docs/
│   ├── README.md            # this file
│   ├── FL_CLIENT_CLI.md     # fl-client CLI reference
│   ├── SECURITY.md          # security architecture + threat model
│   ├── DEPLOYMENT.md        # production deployment guide
│   └── CONTRIBUTING.md      # dev setup, conventions, PR process
└── tests/
    └── integration_test.rs
```

---

## Coordinator (AWS)

The cloud coordinator lives in `coordinator/`. It is a serverless Python application deployed on AWS with:

- **API Gateway** — mTLS enforcement, custom domain
- **Lambda** — 7 functions: health, epoch query, model URL, update URL, update complete, aggregation trigger, audit query
- **DynamoDB** — 4 tables: Epochs, Submissions, Audit, Organizations
- **S3** — model binaries and update binaries
- **ECS Fargate** — aggregation worker (FedAvg + Multi-Krum)
- **CloudWatch** — metrics and structured logs

See [`coordinator/DAEMON_CONNECT.md`](../coordinator/DAEMON_CONNECT.md) for local development setup using Docker Compose.

---

## Documentation

| File | Contents |
|---|---|
| [README.md](README.md) | This file — project overview, quick start, architecture |
| [FL_CLIENT_CLI.md](FL_CLIENT_CLI.md) | Full `fl-client` CLI reference — all commands, flags, exit codes, troubleshooting |
| [SECURITY.md](SECURITY.md) | Threat model, DP math, secure aggregation protocol, TPM/HSM setup, audit log integrity |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Production deployment, systemd, monitoring, log rotation, troubleshooting |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Dev setup, code style, testing guidelines, commit format, PR process |

---

## License

Business Source License 1.1 (BUSL-1.1)

Commercial use requires a license from the authors.  
Contact: `arthsrivastava1@gmail.com`

After **2030-07-01** this software converts to Apache 2.0.
