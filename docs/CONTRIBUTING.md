# Contributing to Sangrah

## Development Setup

### Prerequisites

- Rust 1.75+ — install via `rustup update stable`
- Linux recommended for full test coverage (TPM paths, memory locking)
- Docker (optional) — for local coordinator stack

### Clone and build

```bash
git clone https://github.com/Blazearth/fed-learn-model.git
cd fed-learn-model/federated_learning_model

# Build both binaries
cargo build

# Run all tests
cargo test
```

### Local coordinator (for end-to-end testing)

See [`coordinator/DAEMON_CONNECT.md`](../coordinator/DAEMON_CONNECT.md) for spinning up the local Docker Compose stack and connecting the daemon to it.

---

## Project Structure

```
src/
  main.rs               # fl-client-daemon entry point + orchestration loop
  lib.rs                # module declarations
  config.rs             # Configuration structs
  config/manager.rs     # ConfigManager: load, validate, hot reload
  types.rs              # Shared data structures
  error.rs              # Error type hierarchy (thiserror)
  audit.rs              # Tamper-evident SHA-256 hash-chain audit log
  attestation.rs        # TPM hardware attestation
  certificates.rs       # Certificate management, TPM / HSM / CloudHSM
  checkpoint.rs         # Training checkpoint save and resume
  memory.rs             # mlock + zeroize helpers
  metrics.rs            # Resource monitoring, data drift detection
  model.rs              # Model download, signature verification, rollback
  network.rs            # HTTP client, mTLS, exponential backoff retry
  privacy.rs            # Differential privacy: clipping + Gaussian noise
  scheduler.rs          # Multi-model priority-based job scheduler
  secureagg.rs          # Secure aggregation: ECDH masking, dropout recovery
  supply_chain.rs       # Binary hash verification, SBOM generation
  time_sync.rs          # NTP drift validation
  training.rs           # FedProx training engine, dataset validation
  cli/                  # fl-client binary (human-facing CLI)
    main.rs             # CLI entry point
    args.rs             # clap argument parser
    coordinator.rs      # Slim mTLS coordinator client (reqwest)
    config_loader.rs    # Config path resolution with fallback
    menu.rs             # Interactive dialoguer Select menu
    output.rs           # Colored terminal output (respects NO_COLOR)
    progress.rs         # indicatif progress bars
    state.rs            # SubmissionState with atomic write
    commands/           # One file per subcommand
      whoami.rs / epoch.rs / download.rs / train.rs
      submit.rs / run.rs / init.rs / status.rs / version.rs

tests/
  integration_test.rs   # Integration + property-based tests

config/
  config.example.toml           # Annotated full configuration reference
  rust-client-daemon.service    # systemd unit file
  install.sh                    # Installation script
  fraud_detection.schema.json   # Example dataset schema
  credit_scoring.schema.json    # Example dataset schema

coordinator/
  (Python + AWS SAM coordinator — separate deployment)
  DAEMON_CONNECT.md     # Local dev guide: Docker Compose + daemon

docs/
  README.md             # Project overview, quick start, architecture
  FL_CLIENT_CLI.md      # Full fl-client CLI reference
  SECURITY.md           # Threat model, DP math, SecAgg protocol, TPM setup
  DEPLOYMENT.md         # Production deployment, monitoring, troubleshooting
  CONTRIBUTING.md       # This file
```

---

## Code Style

- **Formatting:** `cargo fmt` — no exceptions
- **Linting:** `cargo clippy -- -D warnings` must be clean before PRs
- **Doc comments:** all `pub` items need `///` doc comments
- **Errors:** use `thiserror` — define module-specific variants in `error.rs`, propagate with `?`
- **Async:** use `tokio`; no blocking calls (`std::thread::sleep`, blocking I/O) in async contexts
- **No `unwrap()`/`expect()`** in production paths — use `?` or explicit error handling with context
- **Sensitive types** must derive or implement `Zeroize`

```bash
# Run before every commit
cargo fmt
cargo clippy -- -D warnings
cargo test
```

---

## Testing

### Unit tests

- Live in `#[cfg(test)] mod tests` at the bottom of the source file
- Test one logical unit per function
- Use `tempfile::tempdir()` for filesystem tests — never write to real paths
- Mock network and hardware (TPM/HSM) — don't require real infrastructure

```bash
cargo test                          # all tests
cargo test privacy                  # specific module
cargo test -- --nocapture           # with stdout
PROPTEST_CASES=500 cargo test       # more iterations
```

### Property-based tests

Use `proptest` for invariant testing. The suite currently has **43 correctness properties** across 100–200 iterations each. When adding one:

- Use `ProptestConfig::with_cases(100)` minimum
- Name functions `prop_<what_is_invariant>`
- Include the requirement reference in the doc comment:

```rust
/// Property 10: After clipping, total L2 norm SHALL NOT exceed max_norm.
/// Validates: Requirements 6.1, 6.4, 6.6
#[test]
fn prop_clipping_enforces_max_norm(...) { ... }
```

### Integration tests

Live in `tests/integration_test.rs`. Use `mockito` for HTTP mocking — no real network calls in CI.

---

## Git Workflow

### Branch naming

```
feat/short-description      # new feature
fix/issue-description       # bug fix
docs/what-is-documented     # documentation only
test/what-is-tested         # tests only
chore/task-description      # tooling, deps, config
```

### Commit format (Conventional Commits)

```
type(scope): subject

Types: feat, fix, test, docs, refactor, chore
Scope: config, network, privacy, secureagg, training, cli, coordinator, docs, etc.
```

Examples:
```
feat(privacy): add privacy budget exhaustion error
fix(network): handle 429 rate limit as retryable error
test(secureagg): add mask cancellation property test
docs(cli): document submit idempotency behaviour
chore(deps): update ring to 0.17.8
```

### Pull request process

1. Branch from `main`
2. Write code + tests
3. `cargo fmt && cargo clippy -- -D warnings && cargo test` — all must pass
4. Open PR with description: what changed, why, how tested
5. Squash or keep individual commits — either is fine as long as history is readable
6. Merge on GitHub after review

---

## Requirements Coverage

All 32 daemon requirements are tracked in `.kiro/specs/rust-client-daemon/requirements.md`.
All 15 CLI requirements are tracked in `.kiro/specs/fl-client-cli/requirements.md`.

When adding a feature, reference the relevant requirement numbers in:
- The source file's module-level doc comment
- The commit message scope

---

## Reporting Bugs

Open a GitHub issue with:
- Rust version (`rustc --version`)
- OS and kernel version
- Minimal reproduction steps
- Relevant log output (`journalctl -u rust-client-daemon -n 100`)

For security vulnerabilities — **do not** open a public issue. Email `arthsrivastava1@gmail.com` directly.
