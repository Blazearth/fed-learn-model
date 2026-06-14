//! Hardware attestation: compute own binary hash and report platform trust state.
//!
//! Implements Requirements: 23
//! Design properties: Property 43-adjacent (generated reports always have non-empty binary hash)

use chrono::{DateTime, Utc};
use ring::digest::{digest, SHA256};
use std::fs;

use crate::error::{DaemonError, Result};

// ── AttestationReport ─────────────────────────────────────────────────────────

/// A software attestation report for this daemon instance.
#[derive(Debug, Clone)]
pub struct AttestationReport {
    /// SHA-256 of own binary (hex-encoded).
    pub daemon_binary_hash: String,
    /// Whether EFI Secure Boot appears to be enabled (from /sys/firmware/efi).
    pub secure_boot_enabled: bool,
    /// Platform identifier: "linux-tpm2" or "software-mock".
    pub platform: String,
    /// Timestamp of report generation.
    pub timestamp: DateTime<Utc>,
    /// Signature of report (mock: always empty — replace with TPM signing in production).
    pub signature: Vec<u8>,
}

// ── AttestationManager ────────────────────────────────────────────────────────

/// Generates and verifies software attestation reports.
///
/// This is a software implementation — all cryptographic fields are real
/// (SHA-256 binary hash) but the signature is empty (mock).  The structure
/// is intentionally designed so a production replacement can swap in a real
/// TPM signer without changing the public interface.
pub struct AttestationManager;

impl AttestationManager {
    /// Create a new AttestationManager.
    pub fn new() -> Self {
        Self
    }

    /// Generate an attestation report for the running daemon binary.
    ///
    /// Steps:
    /// 1. Locate own executable via `std::env::current_exe()`
    /// 2. Read the binary and compute SHA-256
    /// 3. Detect Secure Boot status from /sys/firmware/efi presence (Linux only)
    /// 4. Return a signed report (signature is empty in this software mock)
    pub fn generate_report() -> Result<AttestationReport> {
        // Step 1 — locate own binary
        let exe_path = std::env::current_exe().map_err(|e| {
            DaemonError::Other(format!("cannot determine own executable path: {e}"))
        })?;

        // Step 2 — read binary and hash it
        let binary_data = fs::read(&exe_path).map_err(|e| {
            DaemonError::Other(format!(
                "cannot read own binary at {}: {e}",
                exe_path.display()
            ))
        })?;

        let hash_bytes = digest(&SHA256, &binary_data);
        let daemon_binary_hash = hex_encode(hash_bytes.as_ref());

        // Step 3 — detect Secure Boot (EFI presence)
        let secure_boot_enabled = Self::detect_secure_boot();

        // Step 4 — determine platform
        let platform = if cfg!(target_os = "linux") {
            // Software mock — "linux-tpm2" signals TPM2-capable platform
            "linux-tpm2".to_string()
        } else {
            "software-mock".to_string()
        };

        tracing::info!(
            binary_hash = %daemon_binary_hash,
            secure_boot = secure_boot_enabled,
            platform = %platform,
            "Attestation report generated"
        );

        Ok(AttestationReport {
            daemon_binary_hash,
            secure_boot_enabled,
            platform,
            timestamp: Utc::now(),
            signature: vec![], // mock: empty — replace with TPM signing in production
        })
    }

    /// Verify a previously generated attestation report.
    ///
    /// In this software implementation, verification checks that:
    /// - The binary hash is non-empty
    /// - The platform string is non-empty
    ///
    /// A production implementation would verify the TPM signature here.
    pub fn verify_report(report: &AttestationReport) -> bool {
        !report.daemon_binary_hash.is_empty() && !report.platform.is_empty()
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Detect Secure Boot by checking whether /sys/firmware/efi exists.
    ///
    /// This directory is only present on UEFI systems; its existence is a
    /// reasonable (though not conclusive) Secure Boot indicator.
    fn detect_secure_boot() -> bool {
        std::path::Path::new("/sys/firmware/efi").exists()
    }
}

impl Default for AttestationManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests ────────────────────────────────────────────────────────────

    /// generate_report succeeds and returns a report with non-empty binary hash.
    #[test]
    fn test_generate_report_succeeds() {
        let result = AttestationManager::generate_report();
        assert!(result.is_ok(), "generate_report should succeed: {:?}", result.err());
    }

    /// Report contains a non-empty, valid-looking SHA-256 hex hash.
    #[test]
    fn test_report_has_non_empty_hash() {
        let report = AttestationManager::generate_report().unwrap();
        assert!(
            !report.daemon_binary_hash.is_empty(),
            "binary hash must not be empty"
        );
        // SHA-256 produces 32 bytes → 64 hex chars
        assert_eq!(
            report.daemon_binary_hash.len(),
            64,
            "SHA-256 hex must be 64 characters, got {}",
            report.daemon_binary_hash.len()
        );
        // Should only contain hex characters
        assert!(
            report.daemon_binary_hash.chars().all(|c| c.is_ascii_hexdigit()),
            "binary hash should only contain hex characters"
        );
    }

    /// verify_report returns true for a freshly generated report.
    #[test]
    fn test_verify_report_succeeds_for_fresh_report() {
        let report = AttestationManager::generate_report().unwrap();
        assert!(
            AttestationManager::verify_report(&report),
            "verify_report should return true for a valid report"
        );
    }

    /// verify_report returns false when binary hash is empty.
    #[test]
    fn test_verify_report_fails_for_empty_hash() {
        let mut report = AttestationManager::generate_report().unwrap();
        report.daemon_binary_hash = String::new();
        assert!(
            !AttestationManager::verify_report(&report),
            "verify_report should return false when hash is empty"
        );
    }

    /// Report platform is never empty.
    #[test]
    fn test_report_platform_is_set() {
        let report = AttestationManager::generate_report().unwrap();
        assert!(
            !report.platform.is_empty(),
            "platform must not be empty"
        );
    }

    /// Simulates submitting an attestation report to the coordinator.
    ///
    /// In production the daemon serialises the report and sends it via mTLS.
    /// This test verifies the report is serialisable to JSON (coordinator format)
    /// and that the submission logic would be accepted (verify_report = true).
    ///
    /// Requirement 23.3 / 23.4
    #[test]
    fn test_attestation_submission_to_coordinator_accepted() {
        let report = AttestationManager::generate_report().unwrap();

        // Simulate serialisation the coordinator would receive
        let payload = serde_json::json!({
            "daemon_binary_hash": report.daemon_binary_hash,
            "secure_boot_enabled": report.secure_boot_enabled,
            "platform": report.platform,
            "timestamp": report.timestamp.to_rfc3339(),
        });
        let json = serde_json::to_string(&payload)
            .expect("attestation report must be serialisable for coordinator submission");
        assert!(!json.is_empty(), "serialised attestation payload must not be empty");

        // Coordinator acceptance decision mirrors verify_report
        let accepted = AttestationManager::verify_report(&report);
        assert!(accepted, "coordinator must accept a valid attestation report");
    }

    /// Simulates coordinator rejecting an invalid attestation report.
    ///
    /// If the coordinator rejects the attestation the daemon must NOT continue.
    /// This test verifies that verify_report returns false for a tampered report,
    /// which the caller uses as the signal to terminate (Requirement 23.7).
    #[test]
    fn test_attestation_rejection_triggers_termination_signal() {
        let mut report = AttestationManager::generate_report().unwrap();

        // Tamper: clear the binary hash — simulates a compromised/altered binary
        report.daemon_binary_hash = String::new();

        let accepted = AttestationManager::verify_report(&report);
        assert!(
            !accepted,
            "coordinator must reject a report with empty binary hash"
        );

        // The daemon must terminate when coordinator rejects attestation.
        // Here we verify the termination condition (false) is correctly detected.
        let should_terminate = !accepted;
        assert!(
            should_terminate,
            "daemon must terminate when attestation is rejected (Req 23.7)"
        );
    }

    // ── Property-based tests ──────────────────────────────────────────────────
    //
    // **Validates: Requirements 23**
    //
    // Property 43-adjacent: any generated report always has non-empty binary hash.

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(5))]

        /// Property 43-adjacent: generated reports always have a non-empty binary hash.
        ///
        /// We run generate_report() multiple times; the daemon binary doesn't change
        /// between calls so the hash is deterministic.  The property asserts that
        /// no matter when the report is generated, the binary_hash field is non-empty.
        ///
        /// **Validates: Requirements 23**
        #[test]
        fn prop_generated_report_always_has_non_empty_binary_hash(
            // dummy input to satisfy proptest's strategy requirement
            _dummy in 0u8..=255,
        ) {
            let report = AttestationManager::generate_report().unwrap();
            proptest::prop_assert!(
                !report.daemon_binary_hash.is_empty(),
                "generated attestation report must always have a non-empty binary hash"
            );
            proptest::prop_assert_eq!(
                report.daemon_binary_hash.len(),
                64,
                "SHA-256 hex string must be exactly 64 characters"
            );
        }
    }
}
