# Connecting the Rust Daemon to the Local Coordinator

This guide explains how to configure and connect the `fl-client-daemon` Rust binary to the
local Docker Compose coordinator stack for development and end-to-end testing.

---

## Prerequisites

- Docker Desktop or Docker Engine (v24+) installed and running
- Rust toolchain (1.70+) installed via `rustup`
- `/tmp/test-config.toml` created (see Section 3 below, or copy from `config/config.example.toml`)

---

## 1. Start the Local Coordinator Stack

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator
docker compose up -d --build
```

> Note: modern Docker uses `docker compose` (space), not `docker-compose` (hyphen). Both
> work if you have the legacy `docker-compose` plugin, but prefer `docker compose`.

This brings up four services:

| Service        | Host port | Purpose                                                  |
|----------------|-----------|----------------------------------------------------------|
| DynamoDB Local | 8000      | Local DynamoDB — stores epochs, submissions, audit, orgs |
| LocalStack S3  | 4566      | Local S3-compatible storage for models and updates       |
| setup          | —         | One-shot bootstrap: creates tables, bucket, seeds data   |
| Coordinator    | **8082**  | Flask/gunicorn server wrapping all Lambda handlers       |

The `setup` container runs `scripts/bootstrap.py` automatically. It:
- Creates all 4 DynamoDB tables with GSIs
- Creates the `fl-ingestion-bucket` S3 bucket
- Generates an Ed25519 signing key pair into `coordinator/signing_key/`
- Seeds `org-hospital-a` and `org-hospital-b` into OrgTable
- Seeds and activates epoch 1 for `fraud-detection-v2` with threshold=2

Wait for everything to be healthy:

```bash
docker compose ps
curl -s http://localhost:8082/api/health | python3 -m json.tool
```

Expected response:
```json
{ "status": "ok", "timestamp": "2026-06-18T09:07:39.000Z" }
```

If the coordinator isn't up yet, check its logs:
```bash
docker compose logs coordinator
```

---

## 2. Verify the Stack (optional sanity check)

```bash
# Confirm active epoch is visible
curl -s "http://localhost:8082/api/epochs/active?model_id=fraud-detection-v2" \
  -H "X-Test-Org-Id: org-hospital-a" | python3 -m json.tool

# Confirm model exists in S3
curl -s -X POST http://localhost:8082/api/models/download-url \
  -H "Content-Type: application/json" \
  -H "X-Test-Org-Id: org-hospital-a" \
  -d '{"model_id":"fraud-detection-v2","model_version":"v1"}' | python3 -m json.tool
```

---

## 3. Configure the Daemon for Local Mode

The daemon reads a positional config path argument: `./fl-client-daemon /path/to/config.toml`.
Create `/tmp/test-config.toml`:

```toml
organization_id = "org-hospital-a"   # Must match a seeded org in OrgTable

[coordinator]
base_url = "http://localhost:8082"    # Coordinator host port mapped by docker compose
poll_interval_secs = 10
max_backoff_secs = 60
request_timeout_secs = 30
max_retries = 3

[certificates]
cert_path = "/tmp/fl-daemon/certs/client.pem"
ca_bundle_path = "/tmp/fl-daemon/certs/ca-bundle.pem"
cert_dir = "/tmp/fl-daemon/certs"
rotation_warning_days = 30
check_interval_secs = 3600
[certificates.key_storage]
type = "tpm"                          # lowercase — required by Rust serde config
device_path = "/dev/tpmrm0"

[training]
local_epochs = 1
fedprox_mu = 0.01
framework = "pytorch"                 # lowercase — required by Rust serde config
checkpoint_interval_secs = 600
checkpoint_retention_secs = 86400
loss_tolerance_percent = 20.0
max_gradient_norm = 10.0

[privacy]
enabled = true
epsilon = 1.0
delta = 1.0e-5
clip_threshold = 1.0

[secure_aggregation]
enabled = true
dropout_recovery = true

[resources]
max_cpu_percent = 80.0
max_ram_gb = 8.0
max_disk_gb = 100.0
warning_threshold_percent = 80.0

[storage]
working_dir = "/tmp/fl-daemon"
model_dir = "/tmp/fl-daemon/models"
checkpoint_dir = "/tmp/fl-daemon/checkpoints"
audit_log_path = "/tmp/fl-daemon/audit.log"
model_retention_count = 5

[logging]
level = "info"
log_file = "/tmp/fl-daemon/daemon.log"
json_format = true
tamper_evident = false
blockchain_anchoring = false
anchoring_interval_secs = 3600

[network]
max_concurrent_requests = 10
connection_pooling = true
pool_idle_timeout_secs = 90
stream_threshold_bytes = 10485760

[[models]]
model_id = "fraud-detection-v2"
priority = 1
data_source = "/tmp/fl-daemon/data/fraud_data.parquet"
schema_path = "/tmp/fl-daemon/fraud_detection.schema.json"
```

> **Important config notes:**
> - `framework` and `certificates.key_storage.type` must be **lowercase** — the Rust serde
>   config uses `rename_all = "lowercase"`. Using `"PyTorch"` or `"Tpm"` will cause a parse error.
> - There is no `File` key_storage variant — use `tpm` with a placeholder `device_path`.
>   The daemon skips TPM initialization if the device is absent.
> - `LOCAL_MODE=true` is set inside the coordinator container, not in your daemon config.
>   This makes the coordinator read org identity from the `X-Test-Org-Id` HTTP header rather
>   than from an mTLS certificate. The daemon does not need real certificates locally.

---

## 4. Build and Run the Daemon

```bash
cd /home/blaze/vs-code/federated_learning_model

# Build (debug)
cargo build

# Run — config path is the first positional argument (no --config flag)
RUST_LOG=info ./target/debug/fl-client-daemon /tmp/test-config.toml
```

Release build:
```bash
cargo build --release
RUST_LOG=info ./target/release/fl-client-daemon /tmp/test-config.toml
```

---

## 5. Two-Daemon End-to-End Test Scenario

To simulate two organisations submitting updates to a single round:

**Terminal 1** — Daemon A (`org-hospital-a`):
```bash
RUST_LOG=info ./target/debug/fl-client-daemon /tmp/test-config.toml
```

**Terminal 2** — Daemon B (`org-hospital-b`):
```bash
sed 's/org-hospital-a/org-hospital-b/' /tmp/test-config.toml > /tmp/test-config-b.toml
RUST_LOG=info ./target/debug/fl-client-daemon /tmp/test-config-b.toml
```

Both daemons poll the coordinator, download the current model, train locally, upload their
updates, and call `/api/updates/complete`. When the submission count reaches
`secure_agg_threshold` (default: 2), the aggregation trigger fires automatically.

Note: the aggregation trigger is wired into `update_complete` only when `LOCAL_MODE=true`
and the submission count hits the threshold. In the Docker Compose stack this happens
in-process inside the coordinator container (no separate DynamoDB Streams Lambda needed).

---

## 6. API Endpoints the Daemon Uses

| Method | Endpoint                   | Description                          |
|--------|----------------------------|--------------------------------------|
| GET    | `/api/epochs/active`       | Poll for active epoch metadata       |
| POST   | `/api/models/download-url` | Get pre-signed S3 GET URL for model  |
| POST   | `/api/updates/upload-url`  | Get pre-signed S3 PUT URL for update |
| POST   | `/api/updates/complete`    | Submit completion + update hash      |
| GET    | `/api/audit`               | Query audit log entries              |
| GET    | `/api/health`              | Liveness probe                       |

### Request/Response Field Reference

**`/api/updates/complete` body** — coordinator accepts both `epoch` and `epoch_number`:
```json
{
  "model_id": "fraud-detection-v2",
  "epoch_number": 1,
  "update_hash": "<64-char lowercase hex SHA-256>"
}
```

**`/api/models/download-url` response** — returns both aliases:
```json
{
  "url": "http://localstack:4566/...",
  "download_url": "http://localstack:4566/..."
}
```

**`/api/updates/upload-url` response** — returns both aliases:
```json
{
  "url": "http://localstack:4566/...",
  "upload_url": "http://localstack:4566/..."
}
```

S3 keys are deterministic (no UUID) — `updates/{model_id}/{epoch_number}/{org_id}/update.bin`.
This ensures the aggregator can find the exact object the daemon uploaded.

---

## 7. Logs and Debugging

Daemon logs go to stdout and `/tmp/fl-daemon/daemon.log` (JSON format).

Coordinator logs:
```bash
docker compose logs -f coordinator
```

Inspect DynamoDB state (requires Python with boto3):
```bash
# Run inside the coordinator container where boto3 is available
docker exec fl_coordinator python3 -c "
import boto3, os
ddb = boto3.resource('dynamodb', endpoint_url=os.environ['DYNAMODB_ENDPOINT'],
    region_name='us-east-1', aws_access_key_id='test', aws_secret_access_key='test')
for item in ddb.Table('FederatedEpochTable').scan()['Items']:
    print(item.get('epoch_id'), item.get('status'))
"
```

If you have the AWS CLI installed, you can also query directly:
```bash
AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test \
  aws dynamodb scan --table-name FederatedEpochTable \
  --endpoint-url http://localhost:8000 --region us-east-1

# Same via Makefile shortcut:
make list-epochs
```

---

## 8. Running the Integration Test Suite

All integration and unit tests use `moto` to mock AWS. Docker is **not** required.

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator
source .venv/bin/activate   # or: pip install -r tests/requirements.txt

# Integration tests (full round, auth, audit chain, epoch lock, S3 key consistency, Multi-Krum)
pytest tests/ lambdas/shared/tests/ -v --tb=short

# Rust unit + property tests
cd /home/blaze/vs-code/federated_learning_model
cargo test --lib
```

> Note: run `pytest tests/` and `pytest lambdas/shared/tests/` as separate targets to avoid
> a conftest naming collision between `coordinator/tests/conftest.py` and
> `coordinator/lambdas/tests/conftest.py`.

---

## 9. Aggregation Trigger Behaviour in Local vs AWS Modes

| Mode           | How trigger fires                                                  |
|----------------|--------------------------------------------------------------------|
| LOCAL_MODE     | Called in-process from `aggregation_trigger/app.py` when threshold hit, then calls `aggregator.py` directly |
| AWS (SAM)      | DynamoDB Streams on SubmissionTable → Lambda → `ecs.run_task()` → Fargate container |

The local in-process aggregator (`_run_local_aggregation`) imports `aggregator.py` directly
and runs Multi-Krum in the same Python process. No separate container is needed.
