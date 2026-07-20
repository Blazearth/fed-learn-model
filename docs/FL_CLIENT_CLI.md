# fl-client — Federated Learning CLI

The human-facing command-line tool for participating in a Sangrah federated learning federation. It wraps the full training workflow — epoch polling, model download, local FedProx training, differential privacy, secure aggregation, and update submission — behind simple commands that hospital IT staff and data scientists can run without understanding the underlying cryptography.

---

## Table of Contents

1. [Installation](#installation)
2. [Quick Start](#quick-start)
3. [Configuration](#configuration)
4. [Commands](#commands)
   - [init](#fl-client-init)
   - [whoami](#fl-client-whoami)
   - [epoch](#fl-client-epoch)
   - [download](#fl-client-download)
   - [train](#fl-client-train)
   - [submit](#fl-client-submit)
   - [run](#fl-client-run)
   - [version](#fl-client-version)
5. [Interactive Menu](#interactive-menu)
6. [Global Flags](#global-flags)
7. [Exit Codes](#exit-codes)
8. [File Layout](#file-layout)
9. [Environment Variables](#environment-variables)
10. [Security Notes](#security-notes)
11. [Troubleshooting](#troubleshooting)

---

## Installation

### From source (recommended)

```bash
git clone https://github.com/Blazearth/fed-learn-model.git
cd fed-learn-model/federated_learning_model
cargo build --release
sudo cp target/release/fl-client /usr/local/bin/fl-client
```

### Verify

```bash
fl-client version
# 0.1.0
```

---

## Quick Start

```bash
# 1. First time — create your config
fl-client init

# 2. Check your identity
fl-client whoami

# 3. Run the full pipeline in one command
fl-client run
```

That's it. `run` handles epoch polling, model download, local training, privacy protection, and submission automatically.

---

## Configuration

`fl-client` reads a TOML config file. It searches in this order:

| Priority | Path |
|---|---|
| 1 | Path given with `--config <PATH>` flag |
| 2 | `/etc/fl-daemon/config.toml` |
| 3 | `~/.fl-client/config.toml` |

If none are found, run `fl-client init` to create one interactively.

### Minimal config example

```toml
organization_id = "org-aiims"

[coordinator]
base_url            = "https://coordinator.fed-learn.online"
poll_interval_secs  = 10
max_backoff_secs    = 300
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
epsilon         = 1.0
delta           = 1e-5
clip_threshold  = 1.0

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

> **Certificate key path** — the CLI derives the private key path from `cert_path` by replacing the extension with `.key`. For `org-aiims.pem` it expects `org-aiims.key` in the same directory. This matches how the coordinator PKI scripts issue certificates.

---

## Commands

### `fl-client init`

Interactive setup wizard. Creates `~/.fl-client/config.toml` from prompted inputs. Does not require an existing config — designed for first-time setup.

```
fl-client init — interactive configuration wizard

Organization ID (e.g. org-aiims): org-aiims
Coordinator URL [https://coordinator.fed-learn.online]:
Path to org certificate (.pem): /etc/fl-daemon/certs/org-aiims.pem
Path to CA bundle (.pem): /etc/fl-daemon/certs/ca-bundle.pem
Model ID [fraud-detection-v2]:
Path to local training data (.parquet or .csv): /data/training/records.parquet

✓ Config written to /home/user/.fl-client/config.toml
```

**What it validates before writing:**
- The certificate file exists on disk
- If the output path already exists, asks for overwrite confirmation

**Exit codes:** `0` on success or on user-declined overwrite, `1` if cert file not found or write fails.

---

### `fl-client whoami`

Displays the current organization identity loaded from config. Use this to verify the right config and certificate are active before running a training round.

```
+-----------------+-------------------------------------------------+
| Organization ID | org-aiims                                       |
+-----------------+-------------------------------------------------+
| Coordinator     | https://coordinator.fed-learn.online            |
+-----------------+-------------------------------------------------+
| Cert Path       | /etc/fl-daemon/certs/org-aiims.pem              |
+-----------------+-------------------------------------------------+
| Cert Status     | ✓ Found                                         |
+-----------------+-------------------------------------------------+
```

If the cert file is missing, the Cert Status row shows `⚠ NOT FOUND` as a warning — `whoami` still exits `0`. mTLS commands will fail until the cert is in place.

---

### `fl-client epoch`

Queries the coordinator for the currently active training epoch. Use this to confirm a round is open before running `download` or `train` manually.

```
+---------------+--------------------+
| Epoch         | 4                  |
+---------------+--------------------+
| Model ID      | fraud-detection-v2 |
+---------------+--------------------+
| Model Version | v4                 |
+---------------+--------------------+
| Status        | ACTIVE             |
+---------------+--------------------+
| FedProx μ     | 0.01               |
+---------------+--------------------+
| Privacy ε     | 1                  |
+---------------+--------------------+
| Model Hash    | 8347d46394dc1566   |
+---------------+--------------------+
```

If no epoch is active (coordinator returns 404), prints:
```
No active epoch for model fraud-detection-v2
```
and exits `0`. This is not an error — it means the coordinator is between rounds.

---

### `fl-client download`

Downloads the latest global model from S3 via the coordinator's pre-signed URL API. Verifies the SHA-256 hash of the downloaded file against the epoch metadata before saving.

```
  [████████████████████████████████████████] 0 B/0 B (0s)
✓ Model v4 downloaded and verified (16512 bytes)
```

**What it writes:**
- `{model_dir}/model_v4.npy` — the global model binary
- `{model_dir}/current_epoch.json` — epoch metadata (read by `train`)

**Failure behaviour:**
- If the network drops mid-download → exits `1`, no file written
- If SHA-256 mismatch → deletes the downloaded file, exits `1`

---

### `fl-client train`

Loads the model downloaded by `download`, runs local FedProx training on your dataset, applies Differential Privacy and Secure Aggregation masking, and saves the protected update ready for submission.

```
  Training epoch 3/3 [██████████████████████████████] 0s
+----------------+------------------+
| Final Loss     | 0.8000           |
+----------------+------------------+
| Final Accuracy | 60.00%           |
+----------------+------------------+
| Privacy ε      | 1                |
+----------------+------------------+
| Update SHA-256 | bff1f7da007effbf |
+----------------+------------------+
✓ Training complete — protected update ready for submission.
```

**Privacy stack applied (in order):**

1. **FedProx** — local training with proximal term $\mu$ to prevent client drift on non-IID data
2. **Differential Privacy** — gradient clipping at threshold $C$, then Gaussian noise $\mathcal{N}(0, \sigma^2 C^2 I)$ where $\sigma = \frac{\sqrt{2\ln(1.25/\delta)}}{\varepsilon}$
3. **Secure Aggregation** — ECDH pairwise masking so the coordinator sees only the aggregate, never your individual update

Both DP and SecAgg must succeed. If either fails, `train` deletes any partial output and exits `1`.

**What it writes:**
- `{working_dir}/update.bin` — masked, privacy-protected gradient update
- `{working_dir}/update_meta.json` — `{ epoch_number, sha256 }`

If `download` has not been run first:
```
No epoch metadata found. Run 'fl-client download' first.
```

---

### `fl-client submit`

Uploads the protected update to S3 and notifies the coordinator. Submission state is only recorded locally **after** the coordinator confirms receipt — a failed coordinator call leaves your local state unchanged so you can retry safely.

```
  [████████████████████████████████████████] 16384 B/16384 B (1.2 MiB/s)
✓ Submission recorded — epoch #4
```

**Flow:**
```
1. Load update_meta.json + update.bin from working_dir
2. GET pre-signed S3 upload URL from coordinator (mTLS)
3. PUT update.bin → S3 (streaming, with progress bar)
4. POST /api/updates/complete  { epoch_number, update_hash }
5. Only on HTTP 200: write submission_state.json atomically
```

If the S3 upload succeeds but the coordinator call fails, exits `1` — the coordinator did not record the submission, so retrying `submit` will re-upload and re-notify correctly.

If `train` has not been run first:
```
No update metadata found. Run 'fl-client train' first.
```

---

### `fl-client run`

Runs the full pipeline in one command. This is the normal operational command for scheduled or automated participation.

```
fl-client run
```

**Pipeline:**
```
1. GET active epoch from coordinator
2. Check idempotency — if this epoch is already submitted, exit 0
3. download
4. train
5. submit
```

**Idempotency** — if you run `fl-client run` twice for the same epoch, the second call exits immediately:
```
Epoch 4 already submitted — nothing to do.
```

**Abort on failure** — if any step fails, `run` prints the step name and exits `1` without running subsequent steps. No partial state is left that would confuse a retry.

**Automation example:**
```bash
# Run in a cron job, CI pipeline, or systemd timer
fl-client run && echo "Round complete" || echo "Round failed — check logs"
```

---

### `fl-client version`

```bash
fl-client version
# 0.1.0

fl-client --version
# fl-client 0.1.0
```

---

## Interactive Menu

Run `fl-client` with no arguments to enter the interactive menu — useful for hospital staff who are not comfortable with subcommand names.

```
══════════════════════════════════════════
 Federated Learning Client
 org-aiims · https://coordinator.fed-learn.online
══════════════════════════════════════════

> 1. View Active Epoch
  2. Download Model
  3. Train Model
  4. Submit Update
  5. Run Full Pipeline
  6. View Status
  0. Exit
```

Navigate with arrow keys or type the number. Each action runs the same handler as the equivalent subcommand. The menu redisplays after each action. Press `Ctrl-C` or select `0` to exit cleanly.

**Status screen** (option 6):
```
+---------------------+------------------------------------------+
| Organization        | org-aiims                                |
+---------------------+------------------------------------------+
| Coordinator         | https://coordinator.fed-learn.online     |
+---------------------+------------------------------------------+
| Cert Path           | /etc/fl-daemon/certs/org-aiims.pem       |
+---------------------+------------------------------------------+
| Cert Exists         | Yes                                      |
+---------------------+------------------------------------------+
| Last Submitted Epoch| 4                                        |
+---------------------+------------------------------------------+
```

---

## Global Flags

| Flag | Description |
|---|---|
| `--config <PATH>` | Use this config file instead of the default search order. Bypasses `/etc/fl-daemon/config.toml` and `~/.fl-client/config.toml` entirely. |
| `--version` | Print version and exit. |
| `--help` | Print help for any command. |

The `--config` flag is global — it can appear before any subcommand:
```bash
fl-client --config /opt/myorg/config.toml epoch
fl-client --config /opt/myorg/config.toml run
```

---

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Command completed successfully |
| `0` | `run` — current epoch already submitted (idempotent) |
| `0` | `init` — user declined to overwrite existing config |
| `1` | Config file not found or invalid |
| `1` | mTLS handshake or network failure |
| `1` | Coordinator returned an error |
| `1` | Model file not found (run `download` first) |
| `1` | Update file not found (run `train` first) |
| `1` | SHA-256 hash mismatch on downloaded model |
| `1` | Training engine failure |
| `1` | Privacy or secure aggregation failure |
| `1` | S3 upload failure |
| `1` | Coordinator did not confirm submission |

All fatal errors print a message to `stderr` before exiting `1`. This makes `fl-client` safe to use in shell pipelines:

```bash
fl-client download && fl-client train && fl-client submit
# or just:
fl-client run
```

---

## File Layout

All runtime files live under the paths configured in `[storage]`:

```
{model_dir}/
  model_v4.npy            ← global model downloaded from S3
  current_epoch.json      ← epoch metadata written by 'download', read by 'train'

{working_dir}/
  update.bin              ← protected gradient update written by 'train'
  update_meta.json        ← { epoch_number, sha256 } written by 'train'
  submission_state.json   ← last submitted epoch, written atomically by 'submit'
```

`submission_state.json` format:
```json
{
  "last_submitted_epoch": 4,
  "submitted_at": "2026-07-21T10:32:00Z"
}
```

---

## Environment Variables

These override cert/key file paths when running in cloud environments (Vercel, Docker, CI) where the filesystem is read-only.

| Variable | Description |
|---|---|
| `COORDINATOR_CERT_B64` | Base64-encoded org certificate PEM. Overrides `cert_path` in config. |
| `COORDINATOR_KEY_B64` | Base64-encoded org private key PEM. Overrides the `.key` sibling of `cert_path`. |
| `NO_COLOR` | Set to any value to disable colored output (follows the [NO_COLOR](https://no-color.org/) standard). |

**Setting cert from env (example for Docker):**
```bash
export COORDINATOR_CERT_B64=$(base64 -w 0 org-aiims.pem)
export COORDINATOR_KEY_B64=$(base64 -w 0 org-aiims.key)
fl-client run
```

---

## Security Notes

**Raw data never leaves your machine.** `fl-client` only sends:
- The global model download request (authenticated, read-only)
- The protected gradient update (DP-noised + SecAgg-masked)
- The completion notification with the update hash

**mTLS is enforced on every coordinator call.** Requests without a valid X.509 client certificate from your organization's CA are rejected at the API Gateway level before reaching any Lambda function.

**Differential Privacy** adds calibrated Gaussian noise to gradients before they leave your network. The noise scale is:

$$\sigma = \frac{\sqrt{2\ln(1.25/\delta)}}{\varepsilon}$$

With default settings ($\varepsilon = 1.0$, $\delta = 10^{-5}$), $\sigma \approx 4.75$.

**Secure Aggregation** masks your update with ECDH-derived pairwise masks that cancel exactly when summed across participants. The coordinator sees only the aggregate — never your individual contribution.

**Submission state** is written with an atomic `rename` — a kill signal during `submit` cannot corrupt your state file.

---

## Troubleshooting

### `No config found at /etc/fl-daemon/config.toml or ~/.fl-client/config.toml`

Run `fl-client init` to create one, or pass `--config <path>` explicitly.

### `failed to build mTLS identity — check cert+key paths in config`

The CLI expects `org-aiims.pem` and `org-aiims.key` in the same directory. Check that both files exist:

```bash
ls $(dirname $(grep cert_path ~/.fl-client/config.toml | cut -d'"' -f2))
```

### `No active epoch for model fraud-detection-v2`

The coordinator has no active training round for your model. Check the Sangrah dashboard or contact the federation operator to activate an epoch.

### `Update file size N bytes is outside the allowed range (1024–524288000 bytes)`

Your update is too small. This happens with minimal mock datasets. Use a real training dataset that produces at least 1 KB of gradient data.

### `Hash mismatch` on download

The downloaded model binary doesn't match the coordinator's recorded hash. Could be a network corruption. Run `fl-client download` again — it will overwrite and re-verify.

### Coordinator returns `403 Forbidden`

Your certificate may have expired or your organization may not be registered. Check with the federation operator. Run `fl-client whoami` to confirm which cert is loaded.
