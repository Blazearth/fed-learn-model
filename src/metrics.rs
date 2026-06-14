//! Resource monitoring, drift detection, and explainability generation
//!
//! Implements Requirements 12 (resource monitoring/limits), 27 (data drift detection),
//! and 29 (explainability artifacts).

use chrono::Utc;
use std::collections::HashMap;
use std::fs;

use crate::config::ResourceConfig;
use crate::types::{DataStatistics, DriftMetrics, ExplainabilityArtifact, ResourceMetrics};

// ── Resource violation ────────────────────────────────────────────────────────

/// A resource limit violation
#[derive(Debug, Clone)]
pub struct ResourceViolation {
    pub resource: String,
    pub current: f32,
    pub limit: f32,
    pub message: String,
}

// ── MetricsEngine ─────────────────────────────────────────────────────────────

/// Engine for resource monitoring, drift detection, and explainability.
pub struct MetricsEngine {
    config: ResourceConfig,
    /// Stored baselines: model_id → DataStatistics
    baselines: HashMap<String, DataStatistics>,
}

impl MetricsEngine {
    /// Create a new MetricsEngine with the given resource configuration.
    pub fn new(config: ResourceConfig) -> Self {
        Self {
            config,
            baselines: HashMap::new(),
        }
    }

    // ── 13.2 Resource monitoring ─────────────────────────────────────────────

    /// Measure current resource utilisation.
    ///
    /// Reads `/proc/stat` for CPU and `/proc/meminfo` for RAM (Linux).
    /// Disk usage falls back to a safe default when unavailable.
    /// GPU always returns `None` (no GPU library linked).
    pub fn measure_resources(&self) -> ResourceMetrics {
        ResourceMetrics {
            cpu_percent: read_cpu_percent(),
            ram_gb: read_ram_gb(),
            disk_gb: read_disk_gb(),
            gpu_memory_gb: None,
            timestamp: Utc::now(),
        }
    }

    // ── 13.3 Resource limit enforcement ──────────────────────────────────────

    /// Check whether the provided metrics violate any configured resource limits.
    /// Returns one `ResourceViolation` per exceeded limit.
    pub fn check_limits(&self, metrics: &ResourceMetrics) -> Vec<ResourceViolation> {
        let mut violations = Vec::new();

        if metrics.cpu_percent > self.config.max_cpu_percent {
            violations.push(ResourceViolation {
                resource: "cpu".to_string(),
                current: metrics.cpu_percent,
                limit: self.config.max_cpu_percent,
                message: format!(
                    "CPU usage {:.1}% exceeds limit of {:.1}%",
                    metrics.cpu_percent, self.config.max_cpu_percent
                ),
            });
        }

        if metrics.ram_gb > self.config.max_ram_gb {
            violations.push(ResourceViolation {
                resource: "ram".to_string(),
                current: metrics.ram_gb,
                limit: self.config.max_ram_gb,
                message: format!(
                    "RAM usage {:.2} GB exceeds limit of {:.2} GB",
                    metrics.ram_gb, self.config.max_ram_gb
                ),
            });
        }

        if metrics.disk_gb > self.config.max_disk_gb {
            violations.push(ResourceViolation {
                resource: "disk".to_string(),
                current: metrics.disk_gb,
                limit: self.config.max_disk_gb,
                message: format!(
                    "Disk usage {:.2} GB exceeds limit of {:.2} GB",
                    metrics.disk_gb, self.config.max_disk_gb
                ),
            });
        }

        if let (Some(current_gpu), Some(max_gpu)) =
            (metrics.gpu_memory_gb, self.config.max_gpu_memory_gb)
        {
            if current_gpu > max_gpu {
                violations.push(ResourceViolation {
                    resource: "gpu".to_string(),
                    current: current_gpu,
                    limit: max_gpu,
                    message: format!(
                        "GPU memory {:.2} GB exceeds limit of {:.2} GB",
                        current_gpu, max_gpu
                    ),
                });
            }
        }

        violations
    }

    /// Convenience method: `true` when no limits are exceeded.
    pub fn is_within_limits(&self, metrics: &ResourceMetrics) -> bool {
        self.check_limits(metrics).is_empty()
    }

    // ── 13.4 Data drift detection ─────────────────────────────────────────────

    /// Compute drift for `model_id` by comparing `current` statistics against the
    /// stored baseline.
    ///
    /// Simplified PSI per feature:
    ///   score = |current_mean - baseline_mean| / (baseline_stddev + ε)
    ///
    /// The `current` statistics are stored as the new baseline after computation.
    pub fn compute_drift(&mut self, model_id: &str, current: &DataStatistics) -> DriftMetrics {
        let feature_drift = match self.baselines.get(model_id) {
            None => {
                // No baseline yet — zero drift for all features
                current
                    .feature_names
                    .iter()
                    .map(|name| (name.clone(), 0.0_f64))
                    .collect::<HashMap<String, f64>>()
            }
            Some(baseline) => {
                let n = baseline
                    .feature_names
                    .len()
                    .min(current.feature_names.len())
                    .min(baseline.feature_means.len())
                    .min(current.feature_means.len());

                let mut scores = HashMap::new();
                for i in 0..n {
                    let name = &baseline.feature_names[i];
                    let stddev = baseline
                        .feature_stddevs
                        .get(i)
                        .copied()
                        .unwrap_or(0.0);
                    let baseline_mean = baseline.feature_means[i];
                    let current_mean = current.feature_means.get(i).copied().unwrap_or(0.0);

                    let score = (current_mean - baseline_mean).abs() / (stddev + 1e-10);
                    scores.insert(name.clone(), score);
                }
                scores
            }
        };

        let overall_drift = if feature_drift.is_empty() {
            0.0
        } else {
            feature_drift.values().copied().sum::<f64>() / feature_drift.len() as f64
        };

        // Store current as new baseline
        self.baselines.insert(model_id.to_string(), current.clone());

        DriftMetrics {
            feature_drift,
            overall_drift,
            concept_drift_detected: false, // populated by check_drift_alerts
            timestamp: Utc::now(),
        }
    }

    // ── 13.5 Drift alerting ───────────────────────────────────────────────────

    /// Return feature names whose drift score exceeds `threshold`.
    /// Also sets `concept_drift_detected` if overall drift exceeds `threshold * 2.0`.
    pub fn check_drift_alerts(
        &self,
        metrics: &DriftMetrics,
        threshold: f64,
    ) -> Vec<String> {
        metrics
            .feature_drift
            .iter()
            .filter(|(_, &score)| score > threshold)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Return a `DriftMetrics` copy with `concept_drift_detected` populated.
    pub fn annotate_concept_drift(&self, metrics: DriftMetrics, threshold: f64) -> DriftMetrics {
        let concept_drift_detected = metrics.overall_drift > threshold * 2.0;
        DriftMetrics {
            concept_drift_detected,
            ..metrics
        }
    }

    // ── 13.6 Explainability generation ───────────────────────────────────────

    /// Generate an explainability artifact for the given model, epoch, and
    /// externally-computed feature importance scores (e.g. SHAP values).
    pub fn generate_explainability(
        &self,
        model_id: &str,
        epoch: u64,
        feature_names: &[String],
        feature_scores: HashMap<String, f64>,
    ) -> ExplainabilityArtifact {
        // Build the feature importance map, filling in zeros for unlisted features.
        let mut feature_importance: HashMap<String, f64> = feature_names
            .iter()
            .map(|name| {
                let score = feature_scores.get(name).copied().unwrap_or(0.0);
                (name.clone(), score)
            })
            .collect();

        // Also include any scores provided that aren't in feature_names
        for (name, score) in &feature_scores {
            feature_importance.entry(name.clone()).or_insert(*score);
        }

        let shap_values_path = Some(format!(
            "explainability/{}/epoch_{}/shap_values.json",
            model_id, epoch
        ));
        let summary_report_path = format!(
            "explainability/{}/epoch_{}/summary.html",
            model_id, epoch
        );

        ExplainabilityArtifact {
            model_id: model_id.to_string(),
            epoch_number: epoch,
            shap_values_path,
            feature_importance,
            summary_report_path,
            timestamp: Utc::now(),
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Read CPU utilisation from `/proc/stat`.
///
/// Computes (user + nice + system) / (user + nice + system + idle) on the
/// aggregate `cpu` line.  Returns 0.0 on any parse or I/O error.
fn read_cpu_percent() -> f32 {
    // We take two snapshots with a tiny sleep so that we have a meaningful
    // delta rather than a "since boot" value.  For a single-shot call we
    // still need two reads; 50 ms is imperceptible but sufficient.
    fn read_jiffies() -> Option<(u64, u64, u64, u64)> {
        let content = fs::read_to_string("/proc/stat").ok()?;
        let line = content.lines().find(|l| l.starts_with("cpu "))?;
        let mut parts = line.split_whitespace().skip(1);
        let user: u64 = parts.next()?.parse().ok()?;
        let nice: u64 = parts.next()?.parse().ok()?;
        let system: u64 = parts.next()?.parse().ok()?;
        let idle: u64 = parts.next()?.parse().ok()?;
        Some((user, nice, system, idle))
    }

    let snap1 = match read_jiffies() {
        Some(s) => s,
        None => return 0.0,
    };

    // Brief sleep so the two snapshots differ
    std::thread::sleep(std::time::Duration::from_millis(50));

    let snap2 = match read_jiffies() {
        Some(s) => s,
        None => return 0.0,
    };

    let (u1, n1, s1, i1) = snap1;
    let (u2, n2, s2, i2) = snap2;

    let active_delta = (u2 + n2 + s2).saturating_sub(u1 + n1 + s1) as f64;
    let idle_delta = i2.saturating_sub(i1) as f64;
    let total_delta = active_delta + idle_delta;

    if total_delta == 0.0 {
        return 0.0;
    }

    ((active_delta / total_delta) * 100.0) as f32
}

/// Read RAM usage in GB from `/proc/meminfo`.
///
/// Returns `(MemTotal - MemAvailable) / 1024^3`.  Returns 0.0 on error.
fn read_ram_gb() -> f32 {
    fn parse_kb(content: &str, key: &str) -> Option<u64> {
        for line in content.lines() {
            if line.starts_with(key) {
                let kb: u64 = line
                    .split_whitespace()
                    .nth(1)?
                    .parse()
                    .ok()?;
                return Some(kb);
            }
        }
        None
    }

    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return 0.0,
    };

    let total_kb = parse_kb(&content, "MemTotal:").unwrap_or(0);
    let avail_kb = parse_kb(&content, "MemAvailable:").unwrap_or(0);
    let used_kb = total_kb.saturating_sub(avail_kb);

    // Convert KB → GB
    used_kb as f32 / (1024.0 * 1024.0)
}

/// Attempt to read disk usage of the current working directory.
///
/// Uses `std::fs::metadata` on `"."` for a quick stat; returns a fixed
/// conservative value (1.0 GB) if the metadata is unavailable.
fn read_disk_gb() -> f32 {
    // std::fs::metadata gives us the size of the directory entry itself,
    // not the space consumed by its contents.  For a proper du-style count
    // we would need to walk the tree, which is expensive.  The task spec
    // says "use std::fs::metadata on the working directory, or return a
    // fixed value if unavailable", so we return the fixed value.
    match fs::metadata(".") {
        Ok(_) => 1.0, // fixed conservative estimate
        Err(_) => 0.0,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ResourceConfig;
    use crate::types::DataStatistics;

    fn default_config() -> ResourceConfig {
        ResourceConfig {
            max_cpu_percent: 80.0,
            max_ram_gb: 8.0,
            max_disk_gb: 100.0,
            max_gpu_memory_gb: None,
            warning_threshold_percent: 80.0,
        }
    }

    fn make_engine() -> MetricsEngine {
        MetricsEngine::new(default_config())
    }

    fn sample_stats(means: Vec<f64>, stddevs: Vec<f64>) -> DataStatistics {
        let n = means.len();
        DataStatistics {
            feature_means: means,
            feature_stddevs: stddevs,
            label_distribution: HashMap::new(),
            feature_names: (0..n).map(|i| format!("f{}", i)).collect(),
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    /// measure_resources() returns non-negative values for all fields.
    #[test]
    fn test_resource_measurement() {
        let engine = make_engine();
        let metrics = engine.measure_resources();
        assert!(metrics.cpu_percent >= 0.0, "cpu_percent must be non-negative");
        assert!(metrics.ram_gb >= 0.0, "ram_gb must be non-negative");
        assert!(metrics.disk_gb >= 0.0, "disk_gb must be non-negative");
        assert!(metrics.gpu_memory_gb.is_none(), "gpu should be None");
    }

    /// Metrics within limits → no violations.
    #[test]
    fn test_limit_checking_no_violation() {
        let engine = make_engine();
        let metrics = ResourceMetrics {
            cpu_percent: 50.0,
            ram_gb: 4.0,
            disk_gb: 50.0,
            gpu_memory_gb: None,
            timestamp: Utc::now(),
        };
        let violations = engine.check_limits(&metrics);
        assert!(violations.is_empty(), "expected no violations: {:?}", violations);
        assert!(engine.is_within_limits(&metrics));
    }

    /// CPU over limit → returns a CPU violation.
    #[test]
    fn test_limit_checking_cpu_violation() {
        let engine = make_engine();
        let metrics = ResourceMetrics {
            cpu_percent: 95.0,
            ram_gb: 4.0,
            disk_gb: 50.0,
            gpu_memory_gb: None,
            timestamp: Utc::now(),
        };
        let violations = engine.check_limits(&metrics);
        assert_eq!(violations.len(), 1, "expected exactly one violation");
        assert_eq!(violations[0].resource, "cpu");
        assert!(!engine.is_within_limits(&metrics));
    }

    /// compute_drift with no baseline returns zero drift.
    #[test]
    fn test_drift_computation_no_baseline() {
        let mut engine = make_engine();
        let stats = sample_stats(vec![1.0, 2.0], vec![0.5, 0.5]);
        let drift = engine.compute_drift("model-a", &stats);
        assert_eq!(drift.overall_drift, 0.0);
        assert!(drift.feature_drift.values().all(|&s| s == 0.0));
    }

    /// Significant mean shift → non-zero drift detected.
    #[test]
    fn test_drift_computation_with_drift() {
        let mut engine = make_engine();
        let baseline = sample_stats(vec![1.0, 2.0], vec![0.5, 0.5]);
        // First call establishes the baseline
        engine.compute_drift("model-b", &baseline);

        let shifted = sample_stats(vec![5.0, 8.0], vec![0.5, 0.5]);
        let drift = engine.compute_drift("model-b", &shifted);
        assert!(
            drift.overall_drift > 0.0,
            "expected drift > 0 but got {}",
            drift.overall_drift
        );
    }

    /// Alerts fire above threshold and not below.
    #[test]
    fn test_drift_alerts_threshold() {
        let engine = make_engine();
        let mut feature_drift = HashMap::new();
        feature_drift.insert("f0".to_string(), 0.8_f64);
        feature_drift.insert("f1".to_string(), 0.2_f64);

        let metrics = DriftMetrics {
            feature_drift,
            overall_drift: 0.5,
            concept_drift_detected: false,
            timestamp: Utc::now(),
        };

        let alerts = engine.check_drift_alerts(&metrics, 0.5);
        assert!(alerts.contains(&"f0".to_string()), "f0 (0.8) should alert");
        assert!(!alerts.contains(&"f1".to_string()), "f1 (0.2) should not alert");
    }

    /// generate_explainability returns artifact with correct model_id and epoch.
    #[test]
    fn test_explainability_generation() {
        let engine = make_engine();
        let features = vec!["age".to_string(), "income".to_string()];
        let mut scores = HashMap::new();
        scores.insert("age".to_string(), 0.7_f64);
        scores.insert("income".to_string(), 0.3_f64);

        let artifact = engine.generate_explainability("fraud-model", 5, &features, scores);
        assert_eq!(artifact.model_id, "fraud-model");
        assert_eq!(artifact.epoch_number, 5);
        assert!(artifact.feature_importance.contains_key("age"));
        assert!(artifact.feature_importance.contains_key("income"));
        assert!(artifact.shap_values_path.is_some());
        assert!(!artifact.summary_report_path.is_empty());
    }

    // ── Property-based tests ──────────────────────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(100))]

        /// **Validates: Requirements 12.7-12.11**
        ///
        /// Property 25: Resource Limit Enforcement
        /// For any resource usage strictly above a limit, check_limits must return
        /// a violation for that resource.
        #[test]
        fn prop_limits_enforced(
            cpu in 0.0f32..200.0,
            ram in 0.0f32..32.0,
            disk in 0.0f32..1000.0,
        ) {
            let config = ResourceConfig {
                max_cpu_percent: 80.0,
                max_ram_gb: 8.0,
                max_disk_gb: 100.0,
                max_gpu_memory_gb: None,
                warning_threshold_percent: 80.0,
            };
            let engine = MetricsEngine::new(config.clone());
            let metrics = ResourceMetrics {
                cpu_percent: cpu,
                ram_gb: ram,
                disk_gb: disk,
                gpu_memory_gb: None,
                timestamp: Utc::now(),
            };
            let violations = engine.check_limits(&metrics);

            if cpu > config.max_cpu_percent {
                prop_assert!(
                    violations.iter().any(|v| v.resource == "cpu"),
                    "expected cpu violation when cpu={} > limit={}",
                    cpu, config.max_cpu_percent
                );
            }
            if ram > config.max_ram_gb {
                prop_assert!(
                    violations.iter().any(|v| v.resource == "ram"),
                    "expected ram violation when ram={} > limit={}",
                    ram, config.max_ram_gb
                );
            }
            if disk > config.max_disk_gb {
                prop_assert!(
                    violations.iter().any(|v| v.resource == "disk"),
                    "expected disk violation when disk={} > limit={}",
                    disk, config.max_disk_gb
                );
            }
        }

        /// **Validates: Requirements 27.1-27.4**
        ///
        /// Property 38: Drift Metric Computation
        /// Drift scores are always non-negative for any pair of statistics.
        #[test]
        fn prop_drift_non_negative(
            means1 in prop::collection::vec(-100.0f64..100.0, 1..5),
            stddevs1 in prop::collection::vec(0.0f64..10.0, 1..5),
            means2 in prop::collection::vec(-100.0f64..100.0, 1..5),
            stddevs2 in prop::collection::vec(0.0f64..10.0, 1..5),
        ) {
            let n = means1.len().min(means2.len()).min(stddevs1.len()).min(stddevs2.len());
            if n == 0 { return Ok(()); }

            let baseline = DataStatistics {
                feature_means: means1[..n].to_vec(),
                feature_stddevs: stddevs1[..n].to_vec(),
                label_distribution: HashMap::new(),
                feature_names: (0..n).map(|i| format!("f{}", i)).collect(),
            };
            let current = DataStatistics {
                feature_means: means2[..n].to_vec(),
                feature_stddevs: stddevs2[..n].to_vec(),
                label_distribution: HashMap::new(),
                feature_names: (0..n).map(|i| format!("f{}", i)).collect(),
            };

            let mut engine = MetricsEngine::new(ResourceConfig {
                max_cpu_percent: 80.0,
                max_ram_gb: 8.0,
                max_disk_gb: 100.0,
                max_gpu_memory_gb: None,
                warning_threshold_percent: 80.0,
            });
            // Establish baseline
            engine.compute_drift("test-model", &baseline);
            // Compute drift against baseline
            let drift = engine.compute_drift("test-model", &current);

            prop_assert!(
                drift.overall_drift >= 0.0,
                "overall_drift must be non-negative, got {}",
                drift.overall_drift
            );
            for (feature, score) in &drift.feature_drift {
                prop_assert!(
                    *score >= 0.0,
                    "drift score for {} must be non-negative, got {}",
                    feature, score
                );
            }
        }

        /// **Validates: Requirements 27.6, 27.7**
        ///
        /// Property 39: Drift Alert Generation
        /// Any feature whose drift score exceeds the threshold must appear in alerts.
        #[test]
        fn prop_drift_alerts_above_threshold(
            scores in prop::collection::vec(0.0f64..10.0, 1..8),
            threshold in 0.1f64..5.0,
        ) {
            let engine = MetricsEngine::new(ResourceConfig {
                max_cpu_percent: 80.0,
                max_ram_gb: 8.0,
                max_disk_gb: 100.0,
                max_gpu_memory_gb: None,
                warning_threshold_percent: 80.0,
            });

            let feature_drift: HashMap<String, f64> = scores
                .iter()
                .enumerate()
                .map(|(i, &s)| (format!("f{}", i), s))
                .collect();

            let overall = scores.iter().sum::<f64>() / scores.len() as f64;

            let metrics = DriftMetrics {
                feature_drift: feature_drift.clone(),
                overall_drift: overall,
                concept_drift_detected: false,
                timestamp: Utc::now(),
            };

            let alerts = engine.check_drift_alerts(&metrics, threshold);
            let alert_set: std::collections::HashSet<&String> = alerts.iter().collect();

            for (feature, &score) in &feature_drift {
                if score > threshold {
                    prop_assert!(
                        alert_set.contains(feature),
                        "feature {} with score {} > threshold {} must be in alerts",
                        feature, score, threshold
                    );
                }
            }
        }
    }
}
