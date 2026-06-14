# Contributing

## Development Setup

### Prerequisites

- Rust 1.75+ (`rustup update stable`)
- `cargo` build tool
- `tempfile` crate (already in dev-dependencies)
- Linux recommended for full test coverage (memory locking, TPM paths)

### Clone and build

```bash
git clone https://github.com/your-org/fl-client-daemon.git
cd fl-client-daemon
cargo build
```

### Run tests

```bash
# All tests
cargo test

# Specific module
cargo test privacy
cargo test model::tests::prop_

# With output
cargo test -- --nocapture

# Property tests with more iterations
PROPTEST_CASES=500 cargo test
```

### Generate API documentation

```bash
cargo doc --no-deps --open
```

## Project Structure

```
src/
  lib.rs              # Module declarations and re-exports
  main.rs             # Binary entry point, daemon orchestration loop
  config.rs           # Configuration structs (Req 1)
  config/manager.rs   # ConfigManager: load, validate, hot reload (Req 15)
  types.rs            # Shared data structures
  error.rs            # Error type hierarchy
  audit.rs            # Tamper-evident audit logging (Req 11)
  attestation.rs      # Hardware attestation (Req 23)
  certificates.rs     # Certificate management, TPM/HSM (Req 2, 16, 17)
  checkpoint.rs       # Training checkpoints (Req 25)
  memory.rs           # Memory locking and zeroization helpers (Req 24)
  metrics.rs          # Resource monitoring, drift detection (Req 12, 27, 29)
  model.rs            # Model download, verification, rollback (Req 4, 21, 22, 30)
  network.rs          # HTTP client, mTLS, retry logic (Req 2, 3, 13)
  privacy.rs          # Differential privacy (Req 6)
  scheduler.rs        # Multi-model job scheduler (Req 26)
  secureagg.rs        # Secure aggregation (Req 7, 18)
  supply_chain.rs     # Binary verification, SBOM (Req 31)
  time_sync.rs        # NTP time validation (Req 32)
  training.rs         # FedProx training engine (Req 5, 19, 20, 28)

tests/
  integration_test.rs # Integration and property-based tests

config/
  config.example.toml          # Annotated example configuration
  rust-client-daemon.service   # systemd unit file
  install.sh                   # Installation script
  fraud_detection.schema.json  # Example dataset schema
  credit_scoring.schema.json   # Example dataset schema

docs/
  README.md        # Project overview
  SECURITY.md      # Security architecture
  DEPLOYMENT.md    # Production deployment guide
  CONTRIBUTING.md  # This file
```

## Code Style

- Follow standard Rust idioms (`cargo fmt`, `cargo clippy`)
- All public items must have doc comments
- Errors use `thiserror` — define module-specific variants in `error.rs`
- Async code uses `tokio`; avoid blocking calls in async contexts
- No `unwrap()` or `expect()` in production paths — use `?` or explicit error handling
- Sensitive data types must derive or implement `Zeroize`

### Formatting and linting

```bash
cargo fmt
cargo clippy -- -D warnings
```

## Testing Guidelines

### Unit tests

- Go in a `#[cfg(test)] mod tests` block at the bottom of the source file
- Test one logical unit per function
- Use `tempfile::tempdir()` for filesystem tests — never write to real paths
- Mock network and hardware (TPM/HSM) — don't require real infrastructure

### Property-based tests

- Use `proptest` with at least 100 cases (`ProptestConfig::with_cases(100)`)
- Each property test maps to a named design property (e.g., "Property 32")
- Include the requirement reference in the doc comment
- Keep strategies focused — avoid overly complex combinatorial strategies

### Integration tests

- Live in `tests/integration_test.rs`
- May use `mockito` for HTTP mocking
- Should be deterministic (no real network calls)

## Git Workflow

### Branch naming

```
feature/short-description
fix/issue-description
test/what-is-tested
docs/what-is-documented
```

### Commit format (Conventional Commits)

```
type(scope): subject

Types: feat, fix, test, docs, refactor, chore
Scope: config, network, privacy, secureagg, model, training, attestation, etc.

Examples:
  feat(privacy): add privacy budget exhaustion error
  test(model): add property test for archive retention
  fix(network): handle 429 rate limit as retryable error
  docs(security): document CloudHSM setup steps
```

### Pull request process

1. Branch from `main`
2. Write code + tests
3. `cargo fmt && cargo clippy && cargo test`
4. Open PR with a description covering: what changed, why, how tested
5. All tests must pass; reviewer approval required before merge

## Requirements Coverage

All 32 requirements are tracked in `.kiro/specs/rust-client-daemon/requirements.md`. When adding a feature, reference the relevant requirement numbers in the source file module doc and in commit messages.

All 43 design properties have corresponding property tests. New properties should follow the naming convention: `prop_<property_name>` with a doc comment referencing the property number and requirement.
