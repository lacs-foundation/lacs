//! Sliding-window per-minute rate limiter for the planning loop.
//!
//! `RateLimiter` persists call timestamps to a file so the limit survives
//! process restarts (e.g. the shell being re-opened mid-session).
//!
//! ### Failure mode
//!
//! IO errors on the backing file emit a warning to stderr but do not block the
//! call. Availability is preferred over rate-count precision: a transient
//! filesystem error should never block the user from planning. The warning
//! ensures operators can diagnose degraded rate limiting rather than
//! discovering it only through unexpected API costs.
//!
//! ### Cross-process safety
//!
//! `check_and_consume` holds an in-process `Mutex` lock for the duration of
//! its read-modify-append. This prevents double-counting within a single
//! process but does not protect against concurrent processes writing the same
//! file. LACS typically runs one shell at a time per user, so this is not a
//! practical concern. For deployment scenarios that need cross-process
//! correctness, replace the `Mutex` with an advisory `flock`.

use std::fmt;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Sliding 60-second window rate limiter backed by a plain-text timestamp file.
///
/// Create via [`RateLimiter::new`]; attach to a planner with
/// [`LlmPlanner::with_rate_limiter`](crate::planner::LlmPlanner::with_rate_limiter).
///
/// ### Environment variable
///
/// `LACS_MAX_RPM` overrides `max_per_minute` at runtime:
///
/// ```sh
/// LACS_MAX_RPM=5 lacs "check disk usage"
/// ```
///
/// Values that cannot be parsed as `usize`, or that parse to zero, fall back
/// to the constructor value. Setting `LACS_MAX_RPM=0` is rejected — zero
/// would permanently block all planning calls.
pub struct RateLimiter {
    path: PathBuf,
    max_per_minute: usize,
    /// In-process lock to serialise read-modify-append.
    lock: Mutex<()>,
}

impl fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimiter")
            .field("path", &self.path)
            .field("max_per_minute", &self.max_per_minute)
            .finish_non_exhaustive()
    }
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// - `path`: where timestamps are stored (created on first call if absent).
    /// - `max_per_minute`: calls allowed per 60-second sliding window. Must be
    ///   at least 1; panics otherwise.
    ///   Reads `LACS_MAX_RPM` from the environment and uses it if parseable and
    ///   non-zero, otherwise uses `max_per_minute`.
    ///
    /// # Panics
    ///
    /// Panics if `max_per_minute` is zero.
    pub fn new(path: PathBuf, max_per_minute: usize) -> Self {
        assert!(max_per_minute >= 1, "max_per_minute must be at least 1");
        let effective = std::env::var("LACS_MAX_RPM")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n >= 1)
            .unwrap_or(max_per_minute);
        Self {
            path,
            max_per_minute: effective,
            lock: Mutex::new(()),
        }
    }

    /// Check whether the caller is within the rate window and, if so, record
    /// this call.
    ///
    /// Returns `Ok(())` when the call is allowed, or `Err(retry_after_secs)`
    /// when the window is full. `retry_after_secs` is the number of seconds
    /// until the oldest call in the current window ages out (always >= 1).
    pub fn check_and_consume(&self) -> Result<(), u64> {
        let _guard = self.lock.lock().unwrap_or_else(|e| {
            eprintln!(
                "[lacs-brain] rate-limit: Mutex was poisoned (a prior thread panicked \
                 in the critical section); recovering — timestamp file may be inconsistent"
            );
            e.into_inner()
        });

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| {
                eprintln!(
                    "[lacs-brain] rate-limit: system clock appears to be before \
                     Unix epoch: {e} — rate limit may behave unexpectedly"
                );
                std::time::Duration::ZERO
            })
            .as_secs();
        let window_start = now.saturating_sub(60);

        // Read and parse existing timestamps; silently ignore malformed lines.
        let raw = std::fs::read_to_string(&self.path).unwrap_or_else(|e| {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "[lacs-brain] rate-limit: failed to read timestamp file {}: {e} \
                     — rate limiting is degraded for this call",
                    self.path.display()
                );
            }
            String::new()
        });

        // Separate in-window timestamps from expired ones.
        let (in_window, expired): (Vec<u64>, Vec<u64>) = raw
            .lines()
            .filter_map(|l| l.trim().parse::<u64>().ok())
            .partition(|&t| t >= window_start);

        if in_window.len() >= self.max_per_minute {
            let oldest = in_window.iter().min().copied().unwrap_or(window_start);
            // How long until the oldest call exits the 60-second window.
            let retry_after = (oldest + 60).saturating_sub(now).max(1);
            return Err(retry_after);
        }

        // Write back compacted set (in-window + new timestamp) to keep the
        // file bounded. Expired entries are dropped.
        let _ = expired; // explicitly consumed above
        let mut new_content = String::with_capacity(in_window.len() * 12 + 12);
        for ts in &in_window {
            new_content.push_str(&ts.to_string());
            new_content.push('\n');
        }
        new_content.push_str(&now.to_string());
        new_content.push('\n');

        // Write via a temp file + rename for crash safety, falling back to
        // direct write if the parent directory's temp file creation fails.
        let write_result = if let Some(parent) = self.path.parent() {
            tempfile::NamedTempFile::new_in(parent)
                .and_then(|mut tmp| {
                    tmp.write_all(new_content.as_bytes())?;
                    tmp.persist(&self.path).map_err(|e| e.error)?;
                    Ok(())
                })
        } else {
            std::fs::write(&self.path, &new_content).map_err(Into::into)
        };

        if let Err(e) = write_result {
            eprintln!(
                "[lacs-brain] rate-limit: failed to persist call timestamp to {}: {e} \
                 — this call is allowed but the rate count may be inaccurate",
                self.path.display()
            );
        }

        Ok(())
    }
}
