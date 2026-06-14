# Security Architecture

## Threat Model

The daemon defends against the following threat categories:

| Threat | Control |
|--------|---------|
| Identity spoofing | mTLS client certificates validated against trusted CA |
| Key extraction | Private keys stored in TPM 2.0 / HSM, never in application memory |
| Update poisoning | Server-side Byzantine-resilient aggregation (Multi-Krum) |
| Individual record reconstruction | Differential Privacy (gradient clipping + Gaussian noise) |
| Update inspection by coordinator | Secure Aggregation (ECDH pairwise masking) |
| Compromised binary | Hardware attestation (TPM PCR measurements) |
| Supply chain attack | Binary hash verification + SBOM |
| Audit log tampering | SHA-256 hash chain + Ed25519 signatures |
| Memory scraping | Zeroization of sensitive buffers (zeroize crate) |
| Clock manipulation | NTP validation with configurable strict mode |

## Hardware Key Protection (Requirement 17)

Private keys are **never** stored unencrypted on disk or loaded into application memory.

### TPM 2.0 (recommended)

```toml
[certificates.key_storage]
type = "Tpm"
device_path = "/dev/tpmrm0"
```

The daemon uses the `tss-esapi` crate to perform signing inside the TPM without extracting key material. The TPM resource manager (`/dev/tpmrm0`) should be accessible to the `fl-daemon` user — add it to the `tss` group:

```bash
sudo usermod -aG tss fl-daemon
```

### HSM via PKCS#11

```toml
[certificates.key_storage]
type = "Hsm"
pkcs11_lib = "/usr/lib/softhsm/libsofthsm2.so"
slot_id = 0
```

Tested with SoftHSM2 for development and Thales Luna / Utimaco for production.

### AWS CloudHSM

```toml
[certificates.key_storage]
type = "CloudHsm"
endpoint = "cloudhsm.us-east-1.amazonaws.com"
key_id = "arn:aws:cloudhsm:..."
```

## Certificate Management (Requirements 2, 16)

### Certificate validation

Every mTLS connection validates:
1. Certificate chain against the configured CA bundle
2. Server certificate subject matches expected coordinator hostname
3. Certificate has not expired
4. Certificate was issued by the trusted CA

### Certificate rotation

The daemon watches `certificates.cert_dir` for new `.pem` files. When detected:
1. New certificate is parsed and validated (CA trust, expiration, subject)
2. If valid, the certificate reference is atomically swapped
3. Existing connections continue on the old certificate until they close naturally
4. Rotation event is logged with old and new expiration dates

Configure the warning window (default 30 days):
```toml
[certificates]
rotation_warning_days = 30
```

## Differential Privacy (Requirement 6)

Each model update is protected before leaving the organization:

1. **Gradient clipping** — L2 norm is bounded to `clip_threshold` (sensitivity)
2. **Gaussian noise** — scale = `(sensitivity × √(2 ln(1.25/δ))) / ε`

Privacy budget is tracked across rounds. Configure via:
```toml
[privacy]
enabled = true
epsilon = 1.0      # smaller = stronger privacy
delta = 1.0e-5
clip_threshold = 1.0
```

## Secure Aggregation (Requirement 7)

The coordinator never sees individual plaintext updates:

1. Each participant generates an ephemeral ECDH key pair per round
2. Pairwise masks are derived via ECDH shared secrets + HKDF
3. Masks cancel in aggregate (sum of all masks = 0)
4. The coordinator only recovers the unmasked aggregate

Participant dropout is handled via threshold secret sharing (Requirement 18).

## Hardware Attestation (Requirement 23)

At startup the daemon:
1. Reads its own binary and computes a SHA-256 hash
2. Detects Secure Boot status via EFI firmware path
3. Generates an attestation report
4. Submits the report to the coordinator during authentication
5. Terminates if the coordinator rejects the report

The software implementation uses binary hash measurement as the attestation primitive. A production deployment replaces this with TPM 2.0 PCR quote signing.

## Memory Security (Requirement 24)

Sensitive data is zeroized immediately after use via the `zeroize` crate:

| Data | When zeroed |
|------|-------------|
| Gradient buffers | After masked update is uploaded |
| Cryptographic keys | After signing operation completes |
| ECDH shared secrets | After pairwise mask is derived |
| Secure aggregation masks | After masked update is computed |
| Model binary | After loading into ML framework |

On Linux, sensitive pages can additionally be locked into RAM with `mlock(2)` to prevent swap exposure. This requires `CAP_IPC_LOCK` or an elevated `RLIMIT_MEMLOCK` limit:

```bash
sudo setcap cap_ipc_lock=ep /usr/local/bin/fl-client-daemon
```

## Audit Log Integrity (Requirement 11)

When `tamper_evident = true`:
- Each log entry includes the SHA-256 hash of the previous entry
- Each entry is signed with the configured signing key
- `verify_log_integrity()` can be called to validate the full chain

Optional Hyperledger Fabric anchoring submits aggregate log hashes to the consortium ledger at a configured interval for external auditability.

## Supply Chain Security (Requirement 31)

At startup the daemon:
1. Reads its own binary and computes SHA-256
2. Compares against an optionally configured expected hash
3. Generates an SBOM listing all Cargo dependencies and versions
4. Terminates if binary verification fails

To embed an expected hash:
```bash
sha256sum target/release/fl-client-daemon
# Add the hash to config.toml under [supply_chain] (future feature)
```
