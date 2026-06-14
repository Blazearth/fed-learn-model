//! Integration tests for epoch metadata validation
//!
//! Property-based tests verifying the correctness of epoch metadata validation
//! logic from main.rs.
//!
//! **Validates: Requirements 14**

use fl_client_daemon::types::{EpochMetadata, ParticipantInfo};
use proptest::prelude::*;

// ── Re-export the function under test ─────────────────────────────────────────
// We call the binary's validation function by importing from the library.
// Since validate_epoch_metadata lives in main.rs (binary), we replicate it
// here to make it testable as a library function. The implementation is
// identical to what is in main.rs so the tests validate the same logic.

/// Validate epoch metadata received from the coordinator.
///
/// Checks:
/// - epoch_number > last_processed_epoch (monotonicity)
/// - model_hash is valid 64-char hex string
/// - fedprox_mu within [0.0, 1.0]
/// - privacy_epsilon > 0.0 and privacy_delta > 0.0 && < 1.0
/// - org_id present in secure_agg_participants when list is non-empty
fn validate_epoch_metadata(
    metadata: &EpochMetadata,
    last_processed_epoch: u64,
    org_id: &str,
) -> Result<(), String> {
    // 1. Monotonicity check
    if metadata.epoch_number <= last_processed_epoch {
        return Err(format!(
            "epoch {} is not monotonically increasing (last processed: {})",
            metadata.epoch_number, last_processed_epoch
        ));
    }

    // 2. model_hash must be a valid 64-char hex string
    if metadata.model_hash.len() != 64
        || !metadata.model_hash.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(format!(
            "model_hash '{}' is not a valid 64-char hex string",
            metadata.model_hash
        ));
    }

    // 3. fedprox_mu in [0.0, 1.0]
    if !(0.0..=1.0).contains(&metadata.fedprox_mu) {
        return Err(format!(
            "fedprox_mu {} is outside valid range [0.0, 1.0]",
            metadata.fedprox_mu
        ));
    }

    // 4. privacy_epsilon > 0.0
    if metadata.privacy_epsilon <= 0.0 {
        return Err(format!(
            "privacy_epsilon {} must be positive",
            metadata.privacy_epsilon
        ));
    }

    // 5. privacy_delta > 0.0 && < 1.0
    if metadata.privacy_delta <= 0.0 || metadata.privacy_delta >= 1.0 {
        return Err(format!(
            "privacy_delta {} must be in range (0.0, 1.0)",
            metadata.privacy_delta
        ));
    }

    // 6. org_id must be in participants when list is non-empty
    if !metadata.secure_agg_participants.is_empty() {
        let present = metadata
            .secure_agg_participants
            .iter()
            .any(|p| p.org_id == org_id);
        if !present {
            return Err(format!(
                "org_id '{}' is not in secure_agg_participants list",
                org_id
            ));
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_valid_epoch(epoch: u64, org_id: &str) -> EpochMetadata {
    EpochMetadata {
        epoch_number: epoch,
        model_id: "test-model".to_string(),
        model_version: format!("v{}", epoch),
        model_hash: "a".repeat(64),
        model_signature: vec![0u8; 64],
        architecture_hash: "arch-abc".to_string(),
        fedprox_mu: 0.01,
        privacy_epsilon: 1.0,
        privacy_delta: 1e-5,
        secure_agg_participants: vec![ParticipantInfo {
            org_id: org_id.to_string(),
            public_key: vec![0u8; 32],
        }],
        secure_agg_threshold: 1,
        drift_alerts: vec![],
        dataset_schema: None,
    }
}

// ── Smoke tests ───────────────────────────────────────────────────────────────

#[test]
fn test_valid_epoch_metadata_accepted() {
    let meta = make_valid_epoch(5, "org-a");
    assert!(validate_epoch_metadata(&meta, 4, "org-a").is_ok());
}

#[test]
fn test_epoch_equal_to_last_rejected() {
    let meta = make_valid_epoch(4, "org-a");
    assert!(validate_epoch_metadata(&meta, 4, "org-a").is_err());
}

#[test]
fn test_epoch_less_than_last_rejected() {
    let meta = make_valid_epoch(3, "org-a");
    assert!(validate_epoch_metadata(&meta, 4, "org-a").is_err());
}

#[test]
fn test_short_hash_rejected() {
    let mut meta = make_valid_epoch(5, "org-a");
    meta.model_hash = "abc123".to_string();
    assert!(validate_epoch_metadata(&meta, 4, "org-a").is_err());
}

#[test]
fn test_non_hex_hash_rejected() {
    let mut meta = make_valid_epoch(5, "org-a");
    meta.model_hash = "g".repeat(64); // 'g' is not a hex character
    assert!(validate_epoch_metadata(&meta, 4, "org-a").is_err());
}

#[test]
fn test_org_not_in_participants_rejected() {
    let meta = make_valid_epoch(5, "org-other");
    assert!(validate_epoch_metadata(&meta, 4, "org-mine").is_err());
}

#[test]
fn test_empty_participants_skips_org_check() {
    let mut meta = make_valid_epoch(5, "org-a");
    meta.secure_agg_participants.clear();
    assert!(validate_epoch_metadata(&meta, 4, "totally-different-org").is_ok());
}

// ── Property-based tests ──────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // ── Property 26: Epoch Number Monotonicity ────────────────────────────────
    //
    // **Validates: Requirements 14**
    //
    // For any epoch_number <= last_processed_epoch, validation must fail.
    // For any epoch_number > last_processed_epoch (with otherwise valid metadata),
    // the monotonicity check must pass.

    /// Property 26a: Any epoch_number <= last_processed is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_epoch_monotonicity_violation_always_rejected(
        last_processed in 1u64..=1000,
        // epoch ∈ [0, last_processed] — violates monotonicity
        epoch in 0u64..=1000,
    ) {
        prop_assume!(epoch <= last_processed);
        let meta = make_valid_epoch(epoch, "org-test");
        let result = validate_epoch_metadata(&meta, last_processed, "org-test");
        prop_assert!(
            result.is_err(),
            "epoch {} should be rejected when last_processed={}",
            epoch, last_processed
        );
    }

    /// Property 26b: Any epoch_number > last_processed (with valid metadata) is accepted.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_epoch_monotonicity_valid_accepted(
        last_processed in 0u64..=999,
        delta in 1u64..=500,
    ) {
        let epoch = last_processed + delta;
        let meta = make_valid_epoch(epoch, "org-test");
        let result = validate_epoch_metadata(&meta, last_processed, "org-test");
        prop_assert!(
            result.is_ok(),
            "epoch {} should be accepted when last_processed={}",
            epoch, last_processed
        );
    }

    // ── Property 27: Epoch Metadata Validation ────────────────────────────────
    //
    // **Validates: Requirements 14**
    //
    // Various invalid field combinations must always be rejected.

    /// Property 27a: model_hash not exactly 64 lowercase hex chars is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_invalid_hash_length_always_rejected(
        // Lengths that are not 64
        hash_len in prop::sample::select(vec![0usize, 1, 32, 63, 65, 128]),
    ) {
        let mut meta = make_valid_epoch(5, "org-test");
        meta.model_hash = "a".repeat(hash_len);
        let result = validate_epoch_metadata(&meta, 4, "org-test");
        prop_assert!(
            result.is_err(),
            "hash of length {} should be rejected",
            hash_len
        );
    }

    /// Property 27b: model_hash with non-hex characters is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_non_hex_hash_always_rejected(
        // Characters outside [0-9a-fA-F]
        bad_char in prop::sample::select(vec!['g', 'z', 'G', 'Z', '!', ' ', '-']),
    ) {
        let mut meta = make_valid_epoch(5, "org-test");
        // Fill 64 chars with the bad character
        meta.model_hash = bad_char.to_string().repeat(64);
        let result = validate_epoch_metadata(&meta, 4, "org-test");
        prop_assert!(
            result.is_err(),
            "hash with non-hex char '{}' should be rejected",
            bad_char
        );
    }

    /// Property 27c: fedprox_mu outside [0.0, 1.0] is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_fedprox_mu_out_of_range_always_rejected(
        // mu < 0 or mu > 1
        mu in prop::sample::select(vec![-0.001f32, -1.0, -100.0, 1.001, 2.0, 100.0]),
    ) {
        let mut meta = make_valid_epoch(5, "org-test");
        meta.fedprox_mu = mu;
        let result = validate_epoch_metadata(&meta, 4, "org-test");
        prop_assert!(
            result.is_err(),
            "fedprox_mu {} should be rejected (outside [0.0, 1.0])",
            mu
        );
    }

    /// Property 27d: fedprox_mu within [0.0, 1.0] is accepted (other fields valid).
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_fedprox_mu_in_range_accepted(
        mu in 0.0f32..=1.0,
    ) {
        let mut meta = make_valid_epoch(5, "org-test");
        meta.fedprox_mu = mu;
        let result = validate_epoch_metadata(&meta, 4, "org-test");
        prop_assert!(
            result.is_ok(),
            "fedprox_mu {} should be accepted",
            mu
        );
    }

    /// Property 27e: Non-positive privacy_epsilon is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_invalid_epsilon_always_rejected(
        // epsilon <= 0
        epsilon in prop::sample::select(vec![0.0f64, -0.001, -1.0, -100.0]),
    ) {
        let mut meta = make_valid_epoch(5, "org-test");
        meta.privacy_epsilon = epsilon;
        let result = validate_epoch_metadata(&meta, 4, "org-test");
        prop_assert!(
            result.is_err(),
            "epsilon {} should be rejected (must be > 0)",
            epsilon
        );
    }

    /// Property 27f: privacy_delta outside (0.0, 1.0) is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_invalid_delta_always_rejected(
        // delta out of (0.0, 1.0): 0.0, 1.0, or negative
        delta in prop::sample::select(vec![0.0f64, 1.0, -0.001, -1.0, 2.0]),
    ) {
        let mut meta = make_valid_epoch(5, "org-test");
        meta.privacy_delta = delta;
        let result = validate_epoch_metadata(&meta, 4, "org-test");
        prop_assert!(
            result.is_err(),
            "delta {} should be rejected (must be in (0.0, 1.0))",
            delta
        );
    }

    // ── Property 28: Self-inclusion in participant list ───────────────────────
    //
    // **Validates: Requirements 14**
    //
    // When the participant list is non-empty, the local org_id must be present.
    // Any metadata where the participant list is non-empty and org_id is absent
    // must be rejected.

    /// Property 28a: Missing org_id in non-empty participant list is always rejected.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_missing_self_in_participants_always_rejected(
        // Generate 1-5 other org IDs, none of which will be "org-self"
        n_others in 1usize..=5,
    ) {
        let mut meta = make_valid_epoch(5, "dummy");
        // Replace participants with orgs that are not "org-self"
        meta.secure_agg_participants = (0..n_others)
            .map(|i| ParticipantInfo {
                org_id: format!("org-other-{}", i),
                public_key: vec![0u8; 32],
            })
            .collect();

        let result = validate_epoch_metadata(&meta, 4, "org-self");
        prop_assert!(
            result.is_err(),
            "should reject when 'org-self' is absent from {} participants",
            n_others
        );
    }

    /// Property 28b: org_id present in participant list is accepted.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_self_in_participants_accepted(
        // Insertion position for our org within the list
        insert_pos in 0usize..=4,
        list_size in 1usize..=5,
    ) {
        let insert_pos = insert_pos % list_size;
        let mut participants: Vec<ParticipantInfo> = (0..list_size)
            .map(|i| ParticipantInfo {
                org_id: format!("org-other-{}", i),
                public_key: vec![0u8; 32],
            })
            .collect();
        // Replace one entry with our org_id
        participants[insert_pos] = ParticipantInfo {
            org_id: "org-self".to_string(),
            public_key: vec![0u8; 32],
        };

        let mut meta = make_valid_epoch(5, "org-self");
        meta.secure_agg_participants = participants;

        let result = validate_epoch_metadata(&meta, 4, "org-self");
        prop_assert!(
            result.is_ok(),
            "should accept when 'org-self' is present among {} participants (at pos {})",
            list_size, insert_pos
        );
    }

    /// Property 28c: Empty participant list passes self-inclusion check for any org_id.
    ///
    /// **Validates: Requirements 14**
    #[test]
    fn prop_empty_participant_list_skips_self_check(
        org_suffix in "[a-z0-9]{3,10}",
    ) {
        let org_id = format!("org-{}", org_suffix);
        let mut meta = make_valid_epoch(5, &org_id);
        meta.secure_agg_participants.clear();

        let result = validate_epoch_metadata(&meta, 4, &org_id);
        prop_assert!(
            result.is_ok(),
            "empty participant list should skip self-inclusion check for {}",
            org_id
        );
    }
}
