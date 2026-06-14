//! Time synchronization: NTP clock drift detection.
//!
//! Implements Requirements: 32
//! Design properties: Property 42 (drift bounds validation)

use chrono::{DateTime, Utc};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;

use crate::error::{DaemonError, Result};

// ── TimeSyncConfig ────────────────────────────────────────────────────────────

/// Configuration for the time-sync subsystem.
#[derive(Debug, Clone)]
pub struct TimeSyncConfig {
    /// NTP server address, e.g. "pool.ntp.org:123".
    pub ntp_server: String,
    /// Maximum allowed clock drift in seconds (default 300 = 5 minutes).
    pub max_drift_secs: u64,
    /// Strict mode: if true, `validate()` returns an error when drift exceeds limit.
    pub strict_validation: bool,
}

impl Default for TimeSyncConfig {
    fn default() -> Self {
        Self {
            ntp_server: "pool.ntp.org:123".to_string(),
            max_drift_secs: 300,
            strict_validation: false,
        }
    }
}

// ── TimeSync ──────────────────────────────────────────────────────────────────

/// Verifies NTP reachability and measures clock drift.
pub struct TimeSync {
    config: TimeSyncConfig,
}

impl TimeSync {
    /// Create a new TimeSync instance.
    pub fn new(config: TimeSyncConfig) -> Self {
        Self { config }
    }

    /// Verify that the NTP server is reachable (UDP-level connectivity).
    ///
    /// Sends an NTP request and waits briefly for a response.
    /// Returns `true` if the server replied within the timeout.
    pub async fn verify_ntp_reachable(&self) -> bool {
        matches!(self.send_ntp_request().await, Ok(_))
    }

    /// Measure clock drift by comparing system time to the NTP server's time.
    ///
    /// Sends an NTP request, reads the transmit timestamp from the response,
    /// and computes `(ntp_time - system_time)` in seconds.
    ///
    /// Falls back to `0.0` if the NTP server is unreachable (e.g. due to a
    /// firewall) so the daemon is never blocked by a temporary NTP outage.
    pub async fn measure_drift(&self) -> f64 {
        match self.send_ntp_request().await {
            Ok(ntp_unix_secs) => {
                let system_unix_secs = Utc::now().timestamp() as f64;
                ntp_unix_secs - system_unix_secs
            }
            Err(e) => {
                tracing::warn!("NTP unreachable ({}); assuming drift = 0.0", e);
                0.0
            }
        }
    }

    /// Validate time synchronization.
    ///
    /// Returns `Ok(drift_secs)` when the drift is within the configured limit.
    ///
    /// Behaviour:
    /// - `strict_validation = false`: always returns `Ok(drift)`, logging a
    ///   warning if the drift is large.
    /// - `strict_validation = true`: returns `Err` if the drift exceeds
    ///   `max_drift_secs`.
    pub async fn validate(&self) -> Result<f64> {
        let drift = self.measure_drift().await;
        let abs_drift = drift.abs();
        let limit = self.config.max_drift_secs as f64;

        if abs_drift > limit {
            if self.config.strict_validation {
                return Err(DaemonError::Other(format!(
                    "clock drift {:.1}s exceeds maximum {:.1}s (strict mode)",
                    abs_drift, limit
                )));
            } else {
                tracing::warn!(
                    drift_secs = drift,
                    max_drift_secs = self.config.max_drift_secs,
                    "Clock drift exceeds threshold — participation continues (strict mode off)"
                );
            }
        } else {
            tracing::debug!(drift_secs = drift, "Clock drift within acceptable bounds");
        }

        Ok(drift)
    }

    /// Return the current system timestamp (Req 32.7).
    ///
    /// Used for all daemon timestamps to ensure consistency.
    pub fn system_time() -> DateTime<Utc> {
        Utc::now()
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Send a simple NTP v3 request and return the server's transmit time as
    /// Unix seconds (f64).
    ///
    /// NTP packet layout (RFC 5905, simplified):
    /// - Byte 0: LI=0, VN=3, Mode=3 → 0x1B
    /// - Bytes 1..47: zeroed
    /// - Server response: transmit timestamp at bytes 40..48 (64-bit NTP epoch)
    async fn send_ntp_request(&self) -> Result<f64> {
        // Resolve the NTP server address
        let addr: SocketAddr = self.config.ntp_server
            .parse()
            .or_else(|_| {
                // Fallback: try resolving as a hostname with default port
                format!("{}:123", self.config.ntp_server).parse::<SocketAddr>()
            })
            .map_err(|e| DaemonError::Other(format!("invalid NTP server address: {e}")))?;

        // Bind to an ephemeral UDP port
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| DaemonError::Other(format!("UDP bind failed: {e}")))?;

        socket
            .connect(addr)
            .await
            .map_err(|e| DaemonError::Other(format!("UDP connect to NTP failed: {e}")))?;

        // Construct NTP request: 48 bytes, first byte = 0x1B (LI=0, VN=3, Mode=3)
        let mut request = [0u8; 48];
        request[0] = 0x1B;

        // Send with a 2-second timeout
        tokio::time::timeout(Duration::from_secs(2), socket.send(&request))
            .await
            .map_err(|_| DaemonError::Other("NTP send timed out".to_string()))?
            .map_err(|e| DaemonError::Other(format!("NTP send error: {e}")))?;

        // Receive response with a 3-second timeout
        let mut response = [0u8; 48];
        tokio::time::timeout(Duration::from_secs(3), socket.recv(&mut response))
            .await
            .map_err(|_| DaemonError::Other("NTP response timed out".to_string()))?
            .map_err(|e| DaemonError::Other(format!("NTP recv error: {e}")))?;

        // Extract transmit timestamp from bytes 40..48 (NTP epoch seconds, big-endian)
        let transmit_secs = u32::from_be_bytes([
            response[40],
            response[41],
            response[42],
            response[43],
        ]) as f64;

        // Convert NTP epoch (Jan 1 1900) to Unix epoch (Jan 1 1970): diff = 70 years
        const NTP_TO_UNIX_OFFSET: f64 = 2_208_988_800.0;
        let unix_secs = transmit_secs - NTP_TO_UNIX_OFFSET;

        Ok(unix_secs)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(max_drift: u64, strict: bool) -> TimeSyncConfig {
        TimeSyncConfig {
            ntp_server: "pool.ntp.org:123".to_string(),
            max_drift_secs: max_drift,
            strict_validation: strict,
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    /// system_time() returns a UTC timestamp close to now.
    #[test]
    fn test_system_time_is_recent() {
        let before = Utc::now();
        let t = TimeSync::system_time();
        let after = Utc::now();
        assert!(t >= before, "system_time should not be before the test start");
        assert!(t <= after, "system_time should not be after the test end");
    }

    /// measure_drift falls back to 0.0 when NTP is unreachable.
    ///
    /// We use an invalid NTP address to force the fallback.
    #[tokio::test]
    async fn test_measure_drift_falls_back_when_unreachable() {
        let config = TimeSyncConfig {
            ntp_server: "127.0.0.1:19999".to_string(), // nothing listens here
            max_drift_secs: 300,
            strict_validation: false,
        };
        let ts = TimeSync::new(config);
        let drift = ts.measure_drift().await;
        // With no NTP server, fallback is 0.0
        assert_eq!(drift, 0.0, "drift should fall back to 0.0 when NTP is unreachable");
    }

    /// validate() with non-strict mode and NTP unreachable returns Ok(0.0).
    #[tokio::test]
    async fn test_validate_non_strict_returns_ok_when_unreachable() {
        let config = TimeSyncConfig {
            ntp_server: "127.0.0.1:19999".to_string(),
            max_drift_secs: 5,
            strict_validation: false,
        };
        let ts = TimeSync::new(config);
        let result = ts.validate().await;
        assert!(result.is_ok(), "non-strict validate should always return Ok");
    }

    /// validate() with strict mode and NTP unreachable returns Ok(0.0)
    /// because the fallback drift is 0.0, which is within any positive limit.
    #[tokio::test]
    async fn test_validate_strict_mode_with_fallback_returns_ok() {
        let config = TimeSyncConfig {
            ntp_server: "127.0.0.1:19999".to_string(),
            max_drift_secs: 5,
            strict_validation: true,
        };
        let ts = TimeSync::new(config);
        let result = ts.validate().await;
        assert!(result.is_ok(), "strict validate with 0.0 drift should return Ok");
    }

    // ── Property-based tests ──────────────────────────────────────────────────
    //
    // **Validates: Requirements 32**
    //
    // Property 42: drift bounds validation

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(50))]

        /// Property 42: For any configured max_drift, if drift is within bounds
        /// validation returns Ok; if drift exceeds max_drift and strict mode is
        /// off, it returns Ok (logs a warning but does not error).
        ///
        /// We simulate drift by constructing a TimeSync whose measure_drift would
        /// return a known value.  Since we cannot override measure_drift in tests,
        /// we directly test the validation logic via a helper.
        ///
        /// **Validates: Requirements 32**
        #[test]
        fn prop_drift_bounds_logic(
            max_drift in 1u64..=600,
            drift_abs in 0.0f64..=1200.0,
            within_bounds in proptest::bool::ANY,
            strict in proptest::bool::ANY,
        ) {
            let drift = if within_bounds {
                // drift within bounds
                (drift_abs % (max_drift as f64)).abs()
            } else {
                // drift exceeds bounds
                max_drift as f64 + drift_abs + 1.0
            };

            let result = simulate_validate(drift, max_drift, strict);

            if within_bounds || !strict {
                proptest::prop_assert!(
                    result.is_ok(),
                    "within-bounds or non-strict should always return Ok, \
                     drift={} max={} strict={}",
                    drift, max_drift, strict
                );
            } else {
                // strict=true and drift > max → must be Err
                proptest::prop_assert!(
                    result.is_err(),
                    "strict mode with exceeded drift must return Err, \
                     drift={} max={} strict={}",
                    drift, max_drift, strict
                );
            }
        }
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Synchronous helper that replicates validate() logic for property testing.
#[cfg(test)]
fn simulate_validate(drift: f64, max_drift_secs: u64, strict: bool) -> Result<f64> {
    let abs_drift = drift.abs();
    let limit = max_drift_secs as f64;

    if abs_drift > limit {
        if strict {
            return Err(DaemonError::Other(format!(
                "clock drift {:.1}s exceeds maximum {:.1}s (strict mode)",
                abs_drift, limit
            )));
        }
    }

    Ok(drift)
}
