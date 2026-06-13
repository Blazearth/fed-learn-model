//! Differential privacy implementation: gradient clipping and Gaussian noise.
//!
//! Implements Requirements: 6
//! Design properties: 10 (clipping threshold), 11 (noise scale formula)

use ring::rand::{SecureRandom, SystemRandom};

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
    pub config: PrivacyConfig,
}

impl PrivacyEngine {
    /// Create a new PrivacyEngine from configuration.
    pub fn new(config: PrivacyConfig) -> Self {
        Self { config }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Apply differential privacy to a model update.
    ///
    /// 1. Clip gradients to L2 norm ≤ clip_threshold  (Req 6.1, 6.4, 6.6)
    /// 2. Add Gaussian noise calibrated to (ε, δ)     (Req 6.2, 6.3, 6.7)
    /// 3. Record privacy parameters in update metadata (Req 6.5)
    pub fn apply_privacy(&self, mut update: ModelUpdate) -> Result<ModelUpdate> {
        let clip = self.config.clip_threshold;
        let epsilon = self.config.epsilon;
        let delta = self.config.delta;

        // Step 1: clip gradients
        let _original_norm = self.clip_gradients(&mut update.gradients, clip);

        // Step 2: compute noise scale
        let noise_scale = Self::compute_noise_scale(clip as f32, epsilon, delta);

        // Step 3: add Gaussian noise
        self.add_gaussian_noise(&mut update.gradients, noise_scale as f32)?;

        // Step 4: recompute gradient norm after noise
        let new_norm = compute_l2_norm(&update.gradients);
        update.metadata.gradient_norm = new_norm;

        // Step 5: record privacy parameters
        update.metadata.privacy_params = Some(PrivacyParameters {
            epsilon,
            delta,
            clip_threshold: clip,
            noise_scale,
        });

        tracing::info!(
            epsilon,
            delta,
            clip_threshold = clip,
            noise_scale,
            "Differential privacy applied"
        );

        Ok(update)
    }

    /// Clip gradients so the total L2 norm does not exceed `max_norm`.
    ///
    /// Computes total L2 norm across ALL tensors, if > max_norm scales all by max_norm/total_norm.
    /// Returns original norm before clipping.
    pub fn clip_gradients(&self, gradients: &mut Vec<Vec<f32>>, max_norm: f32) -> f32 {
        let total_norm = compute_l2_norm(gradients);

        if total_norm > max_norm && total_norm > 0.0 {
            let scale = max_norm / total_norm;
            for layer in gradients.iter_mut() {
                for val in layer.iter_mut() {
                    *val *= scale;
                }
            }
        }

        total_norm
    }

    /// Compute noise scale using the Gaussian mechanism formula:
    /// `sensitivity * sqrt(2 * ln(1.25/delta)) / epsilon`
    pub fn compute_noise_scale(sensitivity: f32, epsilon: f64, delta: f64) -> f64 {
        let s = sensitivity as f64;
        let ln_term = (1.25_f64 / delta).ln();
        s * (2.0 * ln_term).sqrt() / epsilon
    }

    /// Add Gaussian N(0, noise_scale²) noise to each gradient element.
    /// Uses ring::rand::SystemRandom for random bytes, converted to Box-Muller Gaussian.
    pub fn add_gaussian_noise(
        &self,
        gradients: &mut Vec<Vec<f32>>,
        noise_scale: f32,
    ) -> Result<()> {
        if noise_scale <= 0.0 {
            return Ok(());
        }

        let rng = SystemRandom::new();

        for layer in gradients.iter_mut() {
            for val in layer.iter_mut() {
                // Box-Muller transform: needs two uniform [0,1] samples
                let u1 = random_uniform(&rng)?;
                let u2 = random_uniform(&rng)?;

                // Box-Muller: Z = sqrt(-2*ln(u1)) * cos(2*pi*u2)
                let z = ((-2.0 * u1.ln()) as f32).sqrt()
                    * (2.0 * std::f32::consts::PI * u2 as f32).cos();
                *val += noise_scale * z;
            }
        }

        Ok(())
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

/// Generate a uniform random f64 in (0, 1) using ring::rand.
fn random_uniform(rng: &SystemRandom) -> Result<f64> {
    let mut bytes = [0u8; 8];
    rng.fill(&mut bytes).map_err(|e| {
        DaemonError::Privacy(PrivacyError::NoiseGenerationFailed(e.to_string()))
    })?;
    let bits = u64::from_le_bytes(bytes);
    // Map to (0, 1) — avoid exact 0 to prevent ln(0)
    let val = (bits as f64 / u64::MAX as f64).max(1e-10);
    Ok(val)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrivacyConfig;

    fn make_engine(epsilon: f64, delta: f64, clip: f32) -> PrivacyEngine {
        PrivacyEngine::new(PrivacyConfig {
            enabled: true,
            epsilon,
            delta,
            clip_threshold: clip,
        })
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

    /// test_gradient_clipping_reduces_norm — gradients with norm 10, clip at 1.0, verify norm ≤ 1.0
    #[test]
    fn test_gradient_clipping_reduces_norm() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        // L2 norm of [6, 8] = 10.0, which exceeds clip=1.0
        let mut gradients = vec![vec![6.0_f32, 8.0_f32]];
        let original_norm = engine.clip_gradients(&mut gradients, 1.0);

        assert!(
            (original_norm - 10.0).abs() < 1e-3,
            "original norm should be ~10.0, got {}",
            original_norm
        );
        let clipped_norm = compute_l2_norm(&gradients);
        assert!(
            clipped_norm <= 1.0 + 1e-4,
            "clipped norm should be ≤1.0, got {}",
            clipped_norm
        );
    }

    /// test_gradient_clipping_preserves_small_norm — gradients with norm 0.5, clip at 1.0, unchanged direction
    #[test]
    fn test_gradient_clipping_preserves_small_norm() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        // L2 norm of [0.3, 0.4] = 0.5, below clip=1.0 → no change
        let mut gradients = vec![vec![0.3_f32, 0.4_f32]];
        let original_values = gradients[0].clone();
        let original_norm = engine.clip_gradients(&mut gradients, 1.0);

        assert!(
            (original_norm - 0.5).abs() < 1e-4,
            "original norm should be ~0.5, got {}",
            original_norm
        );
        // Values should be unchanged
        for (orig, clipped) in original_values.iter().zip(gradients[0].iter()) {
            assert!(
                (orig - clipped).abs() < 1e-6,
                "value should be unchanged: {} vs {}",
                orig,
                clipped
            );
        }
    }

    /// test_noise_scale_formula — verify noise_scale = 1.0 * sqrt(2*ln(1.25/1e-5)) / 1.0 ≈ 4.75
    #[test]
    fn test_noise_scale_formula() {
        // With sensitivity=1.0, epsilon=1.0, delta=1e-5:
        // noise_scale = 1.0 * sqrt(2 * ln(1.25 / 1e-5)) / 1.0
        let expected = (2.0 * (1.25_f64 / 1e-5_f64).ln()).sqrt();
        let actual = PrivacyEngine::compute_noise_scale(1.0, 1.0, 1e-5);

        assert!(
            (actual - expected).abs() < 1e-9,
            "noise scale mismatch: expected {}, got {}",
            expected,
            actual
        );
        // Should be approximately 4.75
        assert!(
            actual > 4.0 && actual < 6.0,
            "noise scale should be ~4.75, got {}",
            actual
        );
    }

    /// test_apply_privacy_sets_params — apply_privacy sets update.metadata.privacy_params correctly
    #[test]
    fn test_apply_privacy_sets_params() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        let update = make_update(vec![vec![0.5_f32, 0.5_f32], vec![0.3_f32, 0.4_f32]]);

        let result = engine.apply_privacy(update);
        assert!(result.is_ok(), "apply_privacy should succeed: {:?}", result.err());

        let noised = result.unwrap();
        assert!(
            noised.metadata.privacy_params.is_some(),
            "privacy params should be set"
        );
        let params = noised.metadata.privacy_params.unwrap();
        assert_eq!(params.epsilon, 1.0);
        assert_eq!(params.delta, 1e-5);
        assert_eq!(params.clip_threshold, 1.0);
        assert!(params.noise_scale > 0.0);
    }

    /// test_apply_privacy_preserves_structure — output gradients have same shape as input
    #[test]
    fn test_apply_privacy_preserves_structure() {
        let engine = make_engine(1.0, 1e-5, 1.0);
        let input_shape: Vec<usize> = vec![3, 5, 2];
        let gradients: Vec<Vec<f32>> = input_shape
            .iter()
            .map(|&sz| vec![0.1_f32; sz])
            .collect();
        let input_structure: Vec<usize> = gradients.iter().map(|l| l.len()).collect();
        let update = make_update(gradients);

        let result = engine.apply_privacy(update).unwrap();
        let output_structure: Vec<usize> = result
            .gradients
            .iter()
            .map(|l| l.len())
            .collect();

        assert_eq!(
            input_structure, output_structure,
            "gradient shape must be preserved after privacy"
        );
    }

    // ── Property-based tests ──────────────────────────────────────────────────
    //
    // **Validates: Requirements 6.1, 6.2, 6.3, 6.4, 6.6, 6.7**
    //
    // Property 10: Gradient Clipping (Req 6.1, 6.4, 6.6)
    // Property 11: Noise Scale Formula (Req 6.2, 6.3, 6.7)

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(100))]

        /// Property 10: After clipping, total L2 norm SHALL NOT exceed max_norm + epsilon.
        ///
        /// **Validates: Requirements 6.1, 6.4, 6.6**
        #[test]
        fn prop_clipping_enforces_max_norm(
            layer1 in proptest::collection::vec(-100.0f32..=100.0f32, 1..=20),
            layer2 in proptest::collection::vec(-100.0f32..=100.0f32, 1..=20),
            max_norm in 0.1f32..=10.0f32,
        ) {
            let engine = make_engine(1.0, 1e-5, max_norm);
            let mut gradients = vec![layer1, layer2];

            // Filter NaN/Inf
            for layer in &mut gradients {
                for v in layer.iter_mut() {
                    if !v.is_finite() { *v = 0.0; }
                }
            }

            engine.clip_gradients(&mut gradients, max_norm);
            let norm_after = compute_l2_norm(&gradients);

            proptest::prop_assert!(
                norm_after <= max_norm + 1e-4,
                "clipped norm {} exceeds max_norm {}",
                norm_after, max_norm
            );
        }

        /// Property 11: Noise scale is always positive for valid epsilon and delta.
        ///
        /// **Validates: Requirements 6.2, 6.3, 6.7**
        #[test]
        fn prop_noise_scale_positive(
            epsilon in 0.1f64..=10.0,
            delta_exp in 2u32..=8,
            sensitivity in 0.1f32..=5.0,
        ) {
            let delta = 1e-5_f64 * (delta_exp as f64);
            proptest::prop_assume!(delta > 0.0 && delta < 1.0);

            let noise_scale = PrivacyEngine::compute_noise_scale(sensitivity, epsilon, delta);
            proptest::prop_assert!(
                noise_scale > 0.0,
                "noise scale must be positive, got {}",
                noise_scale
            );
            proptest::prop_assert!(
                noise_scale.is_finite(),
                "noise scale must be finite, got {}",
                noise_scale
            );
        }

        /// Property: After apply_privacy, gradients change (noise is added to non-zero inputs).
        ///
        /// **Validates: Requirements 6.2**
        #[test]
        fn prop_apply_privacy_changes_gradients(
            layer in proptest::collection::vec(0.5f32..=2.0f32, 2..=10),
        ) {
            let engine = make_engine(1.0, 1e-5, 10.0);
            let original = layer.clone();
            let update = make_update(vec![layer]);

            let result = engine.apply_privacy(update);
            proptest::prop_assert!(result.is_ok());
            let output = result.unwrap();

            // With noise_scale ~4.75 and inputs ~1.0, it's virtually certain some element changes
            let any_changed = output.gradients[0].iter()
                .zip(original.iter())
                .any(|(&out, &inp)| (out - inp).abs() > 1e-6);

            proptest::prop_assert!(
                any_changed,
                "privacy should change at least one gradient element"
            );
        }
    }
}
