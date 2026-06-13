//! Differential privacy implementation: gradient clipping and Gaussian noise.
//!
//! Implements Requirements: 6
//! Design properties: 10 (clipping threshold), 11 (noise scale formula)

use rand::thread_rng;
use rand_distr::{Distribution, Normal};

use crate::config::PrivacyConfig;
use crate::error::{DaemonError, PrivacyError, Result};
use crate::types::{ModelUpdate, PrivacyParameters};

// ── PrivacyEngine ─────────────────────────────────────────────────────────────

/// Applies differential privacy to model updates.
///
/// Uses:
/// - L2 gradient clipping (Req 6.1, 6.4, 6.6)
/// - Gaussian noise calibrated to (ε, δ) privacy budget (Req 6.2, 6.3, 6.7)
pub struct PrivacyEngine {
    epsilon: f64,
    delta: f64,
    clip_threshold: f32,
}

impl PrivacyEngine {
    /// Create a new PrivacyEngine from configuration.
    ///
    /// Returns an error if the privacy budget parameters are invalid.
    pub fn new(config: &PrivacyConfig) -> Result<Self> {
        // Validate epsilon > 0 and delta in (0, 1)
        if config.epsilon <= 0.0 || !config.delta.is_finite()
            || config.delta <= 0.0 || config.delta >= 1.0
        {
            return Err(DaemonError::Privacy(PrivacyError::InvalidBudget {
                epsilon: config.epsilon,
                delta: config.delta,
            }));
        }

        Ok(Self {
            epsilon: config.epsilon,
            delta: config.delta,
            clip_threshold: config.clip_threshold,
        })
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Apply differential privacy to a model update.
    ///
    /// 1. Clip gradients to L2 norm ≤ clip_threshold  (Req 6.1, 6.4, 6.6)
    /// 2. Add Gaussian noise calibrated to (ε, δ)     (Req 6.2, 6.3, 6.7)
    /// 3. Record privacy parameters in update metadata (Req 6.5)
    pub fn apply_privacy(&self, mut update: ModelUpdate) -> Result<ModelUpdate> {
        // Step 1: clip gradients
        let _original_norm = self.clip_gradients(&mut update.gradients, self.clip_threshold);

        // Step 2: compute noise scale using the standard Gaussian mechanism formula
        // noise_scale = (C * sqrt(2 * ln(1.25/δ))) / ε
        let noise_scale = self.noise_scale_formula(self.clip_threshold, self.epsilon, self.delta);

        // Step 3: add Gaussian noise
        self.add_noise(&mut update.gradients, noise_scale)?;

        // Step 4: recompute gradient norm after noise
        let new_norm = compute_l2_norm(&update.gradients);
        update.metadata.gradient_norm = new_norm;

        // Step 5: record privacy parameters (Req 6.5)
        update.metadata.privacy_params = Some(PrivacyParameters {
            epsilon: self.epsilon,
            delta: self.delta,
            clip_threshold: self.clip_threshold,
            noise_scale,
        });

        Ok(update)
    }

    /// Clip gradients so the L2 norm does not exceed `max_norm`.
    ///
    /// Returns the original L2 norm before clipping.
    pub fn clip_gradients(&self, gradients: &mut Vec<Vec<f32>>, max_norm: f32) -> f32 {
        let l2_norm = compute_l2_norm(gradients);

        if l2_norm > max_norm && l2_norm > 0.0 {
            let scale = max_norm / l2_norm;
            for layer in gradients.iter_mut() {
                for val in layer.iter_mut() {
                    *val *= scale;
                }
            }
        }

        l2_norm
    }

    /// Add Gaussian noise N(0, noise_scale²) to each gradient element.
    pub fn add_noise(&self, gradients: &mut Vec<Vec<f32>>, noise_scale: f64) -> Result<()> {
        if noise_scale <= 0.0 {
            return Ok(()); // no noise needed
        }

        let normal = Normal::new(0.0_f64, noise_scale).map_err(|e| {
            DaemonError::Privacy(PrivacyError::NoiseGenerationFailed(e.to_string()))
        })?;

        let mut rng = thread_rng();
        for layer in gradients.iter_mut() {
            for val in layer.iter_mut() {
                let noise = normal.sample(&mut rng) as f32;
                *val += noise;
            }
        }

        Ok(())
    }

    // ── Noise scale formula ───────────────────────────────────────────────────

    /// Compute noise scale: `(C * sqrt(2 * ln(1.25 / δ))) / ε`
    ///
    /// This is the standard Gaussian mechanism noise calibration for (ε, δ)-DP.
    pub fn noise_scale_formula(&self, clip_threshold: f32, epsilon: f64, delta: f64) -> f64 {
        let c = clip_threshold as f64;
        let ln_term = (1.25_f64 / delta).ln();
        c * (2.0 * ln_term).sqrt() / epsilon
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute the L2 (Euclidean) norm across all gradient layers.
pub fn compute_l2_norm(gradients: &[Vec<f32>]) -> f32 {
    let sum_sq: f32 = gradients
        .iter()
        .flat_map(|layer| layer.iter())
        .map(|&v| v * v)
        .sum();
    sum_sq.sqrt()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrivacyConfig;

    fn make_engine(epsilon: f64, delta: f64, clip: f32) -> PrivacyEngine {
        PrivacyEngine::new(&PrivacyConfig {
            enabled: true,
            epsilon,
            delta,
            clip_threshold: clip,
        })
        .unwrap()
    }

    fn make_update(gradients: Vec<Vec<f32>>) -> ModelUpdate {
        use crate::types::UpdateMetadata;
        let norm = compute_l2_norm(&gradients);
        ModelUpdate {
            gradients,
            metadata: UpdateMetadata {
                sample_count: 100,
                training_loss: 0.5,
                training_accuracy: 0.85,
                gradient_norm: norm,
                epoch_duration_secs: 10,
                privacy_params: None,
            },
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_gradient_clipping_reduces_norm() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        // L2 norm of [3, 4] = 5.0, which exceeds clip=1.0
        let mut gradients = vec![vec![3.0_f32, 4.0_f32]];
        let original_norm = engine.clip_gradients(&mut gradients, 1.0);

        assert!((original_norm - 5.0).abs() < 1e-4, "original norm should be ~5.0, got {}", original_norm);
        let clipped_norm = compute_l2_norm(&gradients);
        assert!(
            clipped_norm <= 1.0 + 1e-4,
            "clipped norm should be ≤1.0, got {}",
            clipped_norm
        );
    }

    #[test]
    fn test_gradient_clipping_exact_threshold() {
        let engine = make_engine(1.0, 1e-5, 2.0);
        // L2 norm = 2.0 exactly — no clipping needed
        let mut gradients = vec![vec![2.0_f32, 0.0_f32]];
        let original_norm = engine.clip_gradients(&mut gradients, 2.0);

        assert!((original_norm - 2.0).abs() < 1e-4);
        // Values should be unchanged
        assert!((gradients[0][0] - 2.0).abs() < 1e-4);
        assert!((gradients[0][1] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_noise_scale_formula() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        // With epsilon=1, delta=1e-5, clip=1:
        // noise_scale = sqrt(2 * ln(125000)) / 1
        let expected = (2.0 * (1.25_f64 / 1e-5_f64).ln()).sqrt();
        let actual = engine.noise_scale_formula(1.0, 1.0, 1e-5);
        assert!(
            (actual - expected).abs() < 1e-9,
            "noise scale: expected {}, got {}",
            expected, actual
        );
    }

    #[test]
    fn test_privacy_applied_to_update() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        let gradients = vec![vec![0.5_f32, 0.5_f32], vec![0.3_f32, 0.4_f32]];
        let update = make_update(gradients);

        let result = engine.apply_privacy(update);
        assert!(result.is_ok(), "apply_privacy should succeed: {:?}", result.err());

        let noised = result.unwrap();
        assert!(noised.metadata.privacy_params.is_some(), "privacy params should be set");
        let params = noised.metadata.privacy_params.unwrap();
        assert_eq!(params.epsilon, 1.0);
        assert_eq!(params.delta, 1e-5);
        assert_eq!(params.clip_threshold, 1.0);
        assert!(params.noise_scale > 0.0);
    }

    // ── Property-based tests ──────────────────────────────────────────────────
    //
    // **Validates: Requirements 6.1, 6.2, 6.3, 6.4, 6.6, 6.7**
    //
    // Property 10: Gradient Clipping (Req 6.1, 6.4, 6.6)
    // Property 11: Noise Scale Formula (Req 6.2, 6.3, 6.7)

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(100))]

        /// Property 10: After clipping, L2 norm SHALL NOT exceed the configured threshold.
        ///
        /// **Validates: Requirements 6.1, 6.4, 6.6**
        #[test]
        fn prop_clipped_norm_never_exceeds_threshold(
            // Generate gradient layers with values in a wide range
            layer1 in proptest::collection::vec(-100.0f32..=100.0f32, 1..=20),
            layer2 in proptest::collection::vec(-100.0f32..=100.0f32, 1..=20),
            clip_threshold in 0.1f32..=10.0f32,
        ) {
            let engine = make_engine(1.0, 1e-5, clip_threshold);
            let mut gradients = vec![layer1, layer2];

            // Filter out NaN/Inf
            for layer in &mut gradients {
                for v in layer.iter_mut() {
                    if !v.is_finite() { *v = 0.0; }
                }
            }

            engine.clip_gradients(&mut gradients, clip_threshold);
            let norm_after = compute_l2_norm(&gradients);

            proptest::prop_assert!(
                norm_after <= clip_threshold + 1e-4,
                "clipped norm {} exceeds threshold {}",
                norm_after, clip_threshold
            );
        }

        /// Property 11: The noise scale formula produces the expected value.
        ///
        /// noise_scale = (C * sqrt(2 * ln(1.25/δ))) / ε
        ///
        /// **Validates: Requirements 6.2, 6.3, 6.7**
        #[test]
        fn prop_noise_scale_formula_correct(
            epsilon in 0.1f64..=10.0,
            // delta must be strictly in (0,1)
            delta_mantissa in 1u64..=9,
            delta_exp in 2u32..=8,
            clip in 0.1f32..=5.0f32,
        ) {
            let delta = (delta_mantissa as f64) * 10.0_f64.powi(-(delta_exp as i32));
            proptest::prop_assume!(delta > 0.0 && delta < 1.0);

            let engine = make_engine(epsilon, delta, clip);
            let actual = engine.noise_scale_formula(clip, epsilon, delta);
            let expected = (clip as f64) * (2.0 * (1.25 / delta).ln()).sqrt() / epsilon;

            proptest::prop_assert!(
                (actual - expected).abs() < 1e-9,
                "noise scale formula mismatch: actual={}, expected={}",
                actual, expected
            );
        }
    }
}
