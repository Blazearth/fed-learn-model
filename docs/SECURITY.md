# Security Architecture

Sangrah's security model is layered by design. No single mechanism solves all threat categories — each layer reinforces the others.

```
Layer 5 │ mTLS Identity          │ API Gateway rejects without valid X.509 cert
Layer 4 │ Byzantine Resilience   │ Multi-Krum discards poisoned updates server-side
Layer 3 │ Secure Aggregation     │ ECDH masks hide individual updates from coordinator
Layer 2 │ Differential Privacy   │ Gaussian noise protects individual training records
Layer 1 │ Local Training         │ Raw data never leaves org boundary
```

---

## Threat Model

| Threat | Control |
|---|---|
| Identity spoofing | mTLS — client cert validated against private CA at API Gateway |
| Key extraction | TPM 2.0 / PKCS#11 HSM — keys never loaded into application memory |
| Update poisoning | Multi-Krum server-side Byzantine-resilient aggregation |
| Individual record reconstruction | Differential Privacy — gradient clipping + Gaussian noise |
| Update inspection by coordinator | Secure Aggregation — ECDH pairwise masking, coordinator sees sum only |
| Compromised binary | SHA-256 binary attestation at daemon startup |
| Supply chain attack | SBOM generation + binary hash verification |
| Audit log tampering | SHA-256 hash chain + Ed25519 signatures |
| Memory scraping | `zeroize` crate — all sensitive buffers zeroed after use |
| Clock manipulation | NTP drift validation with configurable strict mode |

---

## Differential Privacy

Each model update is privacy-protected before it leaves the organization.

### Algorithm

**Step 1 — Gradient clipping**

The L2 norm of the gradient is bounded to sensitivity $C$:

$$g' = g \cdot \min\!\left(1,\; \frac{C}{\|g\|_2}\right)$$

This ensures no single training record can influence the update by more than $C$.

**Step 2 — Gaussian noise**

Calibrated noise is added using the Gaussian mechanism:

$$\tilde{g} = g' + \mathcal{N}\!\left(0,\; \sigma^2 C^2 \mathbf{I}\right)$$

The noise scale $\sigma$ is derived from the $(\varepsilon, \delta)$ privacy budget:

$$\sigma = \frac{\sqrt{2\ln(1.25/\delta)}}{\varepsilon}$$

**Default settings** (`epsilon = 1.0`, `delta = 1e-5`): $\sigma \approx 4.75$

**Step 3 — Privacy parameters recorded**

The applied $(\varepsilon, \delta, C, \sigma)$ values are embedded in the update metadata so the coordinator can verify the privacy guarantee was applied.

### Configuration

```toml
[privacy]
enabled        = true
epsilon        = 1.0      # smaller = stronger privacy, less utility
delta          = 1.0e-5
clip_threshold = 1.0      # sensitivity C
```

### What it protects against

- **Gradient inversion attacks** — reconstructing individual training samples from gradient updates
- **Membership inference attacks** — determining whether a specific record was in the training set

---

## Secure Aggregation

The coordinator never sees individual organization updates in plaintext.

### Protocol

**Round setup**

Each participant generates an ephemeral ECDH key pair per training round:
- Private key: 32 random bytes
- Public key: SHA-256(private_seed || "pubkey")

**Pairwise mask derivation**

For each pair of participants (A, B), a shared secret is derived deterministically:

```
shared_secret = SHA-256(sort(pub_A, pub_B))
```

Both parties compute the same value without communicating, because the keys are sorted canonically.

HKDF-SHA256 expands the shared secret into a mask of the required length.

**Mask application**

$$\text{masked\_update}_i = \tilde{g}_i + \sum_{j \neq i} \text{sign}(i, j) \cdot m_{ij}$$

where $\text{sign}(i, j) = +1$ if $\text{org\_id}_i < \text{org\_id}_j$ (lexicographic), else $-1$.

**Cancellation at server**

When the coordinator sums all masked updates, the pairwise masks cancel exactly:

$$\sum_i \text{masked\_update}_i = \sum_i \tilde{g}_i + \sum_{i} \sum_{j \neq i} \text{sign}(i, j) \cdot m_{ij} = \sum_i \tilde{g}_i$$

The coordinator recovers only the aggregate — never any individual contribution.

### Dropout recovery

If a participant drops out mid-round after committing their masked update, the masks from that participant no longer cancel. Recovery uses threshold secret sharing:

- Each participant pre-shares their pairwise secrets with a threshold $t$ of $n$ other participants
- If the dropout count stays below $n - t$, surviving participants can reconstruct the missing masks
- The aggregation completes correctly without the dropout's raw gradients

Configure the threshold:
```toml
[secure_aggregation]
enabled          = true
dropout_recovery = true
threshold        = 2      # minimum participants needed for recovery
```

---

## Byzantine-Resilient Aggregation (Multi-Krum)

A malicious participant can submit poisoned updates. Multi-Krum filters outliers before aggregation.

### Algorithm

For each submitted update $g_i$, compute a score:

$$s_i = \sum_{j \in \mathcal{N}(i)} \|g_i - g_j\|^2$$

where $\mathcal{N}(i)$ are the $n - f - 2$ nearest neighbors of $g_i$ (with $f$ = assumed Byzantine count).

Updates with the lowest scores (closest to the honest cluster) are selected for aggregation. High-scoring updates are discarded.

This runs on the AWS ECS Fargate aggregation worker, not on client machines.

---

## Hardware Key Protection

Private keys are **never** stored unencrypted on disk or loaded into application memory.

### TPM 2.0 (recommended)

```toml
[certificates.key_storage]
type        = "tpm"
device_path = "/dev/tpmrm0"
```

The daemon signs using `tss-esapi` — signing happens inside the TPM, key material is never extracted. The TPM resource manager must be accessible to the `fl-daemon` user:

```bash
sudo usermod -aG tss fl-daemon
```

### PKCS#11 HSM

```toml
[certificates.key_storage]
type       = "hsm"
pkcs11_lib = "/usr/lib/softhsm/libsofthsm2.so"
slot_id    = 0
```

Tested with SoftHSM2 (dev), Thales Luna, and Utimaco (production).

### AWS CloudHSM

```toml
[certificates.key_storage]
type     = "cloudhsm"
endpoint = "cloudhsm.us-east-1.amazonaws.com"
key_id   = "arn:aws:cloudhsm:..."
```

---

## Certificate Management

### Validation

Every mTLS connection validates:
1. Certificate chain against the configured CA bundle
2. Certificate has not expired
3. Certificate was issued by the trusted CA

### Rotation

The daemon watches `cert_dir` for new `.pem` files. On detection:
1. New cert parsed and validated
2. Certificate reference atomically swapped (no restart needed)
3. Existing connections drain naturally on the old cert
4. Rotation event logged with old and new expiry dates

Configure the expiry warning window:
```toml
[certificates]
rotation_warning_days = 30
```

### Key path convention

The CLI (`fl-client`) derives the private key path from `cert_path` by replacing the extension:

```
cert_path = /etc/fl-daemon/certs/org-aiims.pem
key_path  = /etc/fl-daemon/certs/org-aiims.key   ← derived automatically
```

---

## Hardware Attestation

At daemon startup:

1. Reads own binary, computes SHA-256
2. Detects Secure Boot status via EFI firmware path
3. Generates attestation report
4. Submits report to coordinator during authentication
5. Terminates if coordinator rejects the report

In production, replace the software hash measurement with TPM 2.0 PCR quote signing for hardware-backed attestation.

---

## Memory Security

All sensitive data is zeroed immediately after use via the `zeroize` crate:

| Data | Zeroed when |
|---|---|
| Gradient buffers | After masked update is uploaded to S3 |
| Private key material | After signing operation completes |
| ECDH shared secrets | After pairwise mask is derived |
| Secure aggregation masks | After masked update is computed |
| Model binary | After loading into training engine |

On Linux, sensitive pages can be locked into RAM to prevent swap exposure:

```bash
# Requires CAP_IPC_LOCK or elevated RLIMIT_MEMLOCK
sudo setcap cap_ipc_lock=ep /usr/local/bin/fl-client-daemon
```

---

## Audit Log Integrity

When `tamper_evident = true`, every audit log entry includes:

- A SHA-256 hash of the previous entry (`previous_hash`)
- An Ed25519 signature over the full entry

The chain: $H_n = \text{SHA-256}(H_{n-1} \;\|\; \text{entry}_n)$

Any retroactive modification to any entry breaks the chain and is immediately detectable. The audit log covers: `UPDATE_SUBMITTED`, `AGGREGATION_TRIGGERED`, `MODEL_PUBLISHED` events, plus identity and certificate events.

Optional Hyperledger Fabric anchoring submits aggregate log hashes to the consortium ledger at a configurable interval for external, cross-organizational auditability.

```toml
[logging]
tamper_evident       = true
blockchain_anchoring = true
anchoring_interval_secs = 3600
```

---

## Supply Chain Security

At startup, the daemon:

1. Reads own binary and computes SHA-256
2. Compares against an optionally configured expected hash
3. Generates an SBOM listing all Cargo dependencies and versions
4. Terminates if binary verification fails

This prevents tampered binaries — whether from a compromised build pipeline or a malicious update — from participating in the federation.

---

## Network Security

- All coordinator communication is **outbound-only** from the organization
- No inbound ports need to be opened on the organization's firewall
- All connections use **TLS 1.2+** with `rustls` (no OpenSSL dependency)
- S3 uploads/downloads use **pre-signed URLs** — the coordinator API never handles large payloads
- IPv6 is disabled by default (`family: 4`) to prevent IPv6-only routing issues

---

## Reporting Vulnerabilities

Do not file public GitHub issues for security vulnerabilities.

Email: `arthsrivastava1@gmail.com`

Include: description, reproduction steps, affected versions, and your assessment of severity. We aim to respond within 48 hours.
