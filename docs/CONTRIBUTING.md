# Contributing

---

## Development setup

### Prerequisites

- Rust 1.75+ via `rustup`
- Linux recommended (memory locking + TPM paths used in some tests)
- Docker (optional — for local coordinator stack)

```bash
rustup update stable
git clone https://github.com/Blazearth/fed-learn-model.git
cd fed-learn-model/federated_learning_model
cargo build
```

### Run all tests

```bash
cargo test

# Verbose output
cargo test -- --nocapture

# Specific module
cargo test privacy
cargo test secureagg
cargo test training

# More property iterations
PROPTEST_CASES=500 cargo test
```

The test suite has **171 tests** — unit, property-based, and integration. All must pass before a PR is merged.

### Lint and format

```bash
cargo fmt
cargo clippy -- -D warnings
```

Both are required. A PR with clippy warnings will not be merged.

### Generate API docs

```bash
cargo doc --no-deps --open
```

---

## Project structure

```
src/
  main.rs              # fl-client-daemon entry point + orchestration loop
  lib.rs               # module declarations and re-exports
  config.rs            # configuration structs
  config/manager.rs    # load, validate, hot reload (SIGHUP)
  types.rs             # shared data structures (EpochMetadata, ModelUpdate, etc.)
  error.rs             # error type hierarchy (thiserror)
  audit.rs             # tamper-evident SHA-256 hash-chain audit log
  attestation.rs       # TPM hardware attestation at startup
  certificates.rs      # X.509 cert management, TPM/HSM key storage
  checkpoint.rs        # training checkpoint save and resume
  memory.rs            # mlock + zeroize helpers
  metrics.rs           # resource monitoring, data drift detection
  model.rs             # model download, Ed25519 signature verification, rollback
  network.rs           # HTTP client, mTLS, exponential backoff retry
  privacy.rs           # differential privacy — clipping + Gaussian noise
  scheduler.rs         # multi-model priority scheduler with preemption
  secureagg.rs         # secure aggregation — ECDH masking, dropout recovery
  supply_chain.rs      # binary hash verification + SBOM generation
  time_sync.rs         # NTP drift validation
  training.rs          # FedProx training engine, quality gates, data validation
  cli/                 # fl-client binary (separate [[bin]] entry)
    main.rs            # CLI entry point — dispatch to commands or interactive menu
    args.rs            # clap argument parser
    coordinator.rs     # mTLS CoordinatorClient for CLI use
    config_loader.rs   # config path resolution (3-level fallback)
    menu.rs            # interactive dialoguer Select menu
    output.rs          # colored terminal output helpers
    progress.rs        # indicatif progress bars
    state.rs           # SubmissionState with atomic write
    commands/          # one file per subcommand
      whoami.rs
      epoch.rs
      download.rs
      train.rs
      submit.rs
      run.rs
      init.rs
      status.rs
      version.rs

tests/
  integration_test.rs  # integration and property-based tests

config/
  config.example.toml           # fully annotated example configuration
  rust-client-daemon.service    # systemd unit file
  install.sh                    # system installer script
  fraud_detection.schema.json   # example dataset schema
  credit_scoring.schema.json    # example dataset schema

docs/
  README.md          # project overview, architecture, quick start
  FL_CLIENT_CLI.md   # fl-client CLI full reference
  SECURITY.md        # security architecture and threat model
  DEPLOYMENT.md      # production deployment guide
  CONTRIBUTING.md    # this file

coordinator/           # AWS cloud coordinator (Python + SAM)
  lambdas/             # 7 Lambda functions
  aggregation/         # ECS Fargate aggregation worker
  scripts/             # operator scripts (cert issuance, epoch management)
  DAEMON_CONNECT.md    # local dev guide — connecting daemon to Docker Compose
```

---

## Code conventions

### Error handling

- Use `thiserror` for all library errors — define variants in `error.rs`
- Never use `unwrap()` or `expect()` in production paths — use `?` or explicit matching
- CLI commands return `ExitCode` — print errors via `output::error()` before returning `ExitCode::FAILURE`

### Async

- All async code uses Tokio
- No blocking calls inside async functions — use `tokio::task::spawn_blocking` if needed

### Sensitive data

- Any type holding key material, gradient buffers, or masks must derive or implement `Zeroize`
- Call `zeroize()` explicitly on temporary sensitive buffers after use (don't rely on `Drop`)

### Modules

- All public items must have doc comments (`///`)
- Keep `mod.rs` files minimal — just `pub mod` declarations
- Tests live in `#[cfg(test)] mod tests` at the bottom of the source file

---

## Testing guidelines

### Unit tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something_specific() {
        // Arrange
        // Act
        // Assert
    }
}
```

- One logical unit per test function
- Use `tempfile::tempdir()` for any filesystem interaction — never real paths
- Mock network and hardware — no real coordinator calls in unit tests

### Property-based tests

```rust
proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(100))]

    /// Property N: description of the invariant.
    /// Validates: Requirements X.Y
    #[test]
    fn prop_invariant_name(input in strategy) {
        prop_assert!(condition);
    }
}
```

- At least 100 cases per property
- Include the requirement reference in the doc comment
- Name: `prop_<invariant>` — matches the design property numbering in the spec

### Integration tests

- Live in `tests/integration_test.rs`
- Use `mockito` for HTTP mocking — no real network calls
- Must be deterministic

---

## Git workflow

### Branch naming

```
feat/short-description      # new feature
fix/what-is-fixed           # bug fix
docs/what-is-documented     # documentation only
refactor/what-is-changed    # no behavior change
test/what-is-tested         # tests only
chore/what-is-done          # tooling, deps, CI
```

### Commit format (Conventional Commits)

```
type(scope): subject line under 72 chars

Optional longer body explaining WHY, not what.
Reference requirements: Req 5.3, Req 7.

Types:  feat, fix, test, docs, refactor, chore
Scope:  config, network, privacy, secureagg, model, training,
        attestation, cli, coordinator, etc.
```

Examples:
```
feat(privacy): add privacy budget exhaustion error
fix(network): treat 429 rate limit as retryable
test(secureagg): add property test for mask cancellation
docs(cli): document --config flag fallback order
chore(deps): pin ring to 0.17.8
```

### Pull request process

1. Branch from `main`
2. Write code + tests (`cargo fmt && cargo clippy && cargo test`)
3. Open PR with:
   - What changed and why
   - How it was tested
   - Any requirements references (e.g., "Implements Req 6.3")
4. All CI checks must pass
5. Reviewer approval required before merge
6. Merge via GitHub — not locally

---

## Requirements and properties

All 32 daemon requirements are tracked in `.kiro/specs/rust-client-daemon/requirements.md`.  
All 15 CLI requirements are in `.kiro/specs/fl-client-cli/requirements.md`.

When adding a feature:
- Reference the relevant requirement numbers in the source file module doc
- Reference them in commit messages
- Add a property test for any non-trivial invariant

The 43 existing design properties all have corresponding property tests named `prop_<property_name>`. New properties follow the same pattern.
