//! Unlocked-wallet session and password-attempt limiting.
//!
//! While unlocked, the session holds the decrypted [`WalletEntropy`] (wiped on
//! drop) and a cache of derived keys, so syncing and address display do not
//! re-run Argon2id or re-derive keys. The password itself is not kept; it is
//! asked for again for sending and for revealing the seed phrase.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use neptune_wallet::wallet_entropy::WalletEntropy;
use serde::Deserialize;
use serde::Serialize;

use crate::keys::KeyCache;

/// Default auto-lock: this many seconds without user activity.
pub(crate) const DEFAULT_AUTO_LOCK_SECS: u64 = 180;

/// Allowed range for the user-chosen auto-lock timeout.
pub(crate) const MIN_AUTO_LOCK_SECS: u64 = 30;
pub(crate) const MAX_AUTO_LOCK_SECS: u64 = 3600;

/// Failed password attempts allowed before the first cooldown.
pub(crate) const MAX_PASSWORD_ATTEMPTS: u32 = 5;

/// First cooldown; doubles with every further failure, up to the maximum.
const FIRST_COOLDOWN_SECS: u64 = 60;
const MAX_COOLDOWN_SECS: u64 = 3600;

/// Seconds on a clock that keeps running while the device is asleep.
///
/// `std::time::Instant` is `CLOCK_MONOTONIC` on Linux/Android, which stops
/// during suspend, so a phone left asleep for an hour would still count as
/// "active" for the auto-lock. `CLOCK_BOOTTIME` includes suspended time and,
/// unlike wall-clock time, cannot be changed by the user.
pub(crate) fn activity_clock_secs() -> u64 {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid, writable timespec.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } == 0 {
            return ts.tv_sec as u64;
        }
    }
    epoch_secs()
}

pub(crate) fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) struct Session {
    pub(crate) entropy: Arc<WalletEntropy>,
    pub(crate) keys: Arc<KeyCache>,
    last_activity: u64,
}

impl Session {
    pub(crate) fn new(entropy: WalletEntropy) -> Self {
        Self {
            entropy: Arc::new(entropy),
            keys: Arc::new(KeyCache::default()),
            last_activity: activity_clock_secs(),
        }
    }

    pub(crate) fn touch(&mut self) {
        self.last_activity = activity_clock_secs();
    }

    pub(crate) fn is_expired(&self, timeout_secs: u64) -> bool {
        activity_clock_secs().saturating_sub(self.last_activity) > timeout_secs
    }
}

/// App-wide settings kept by the backend (not tied to a particular wallet).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AppSettings {
    pub(crate) auto_lock_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_lock_secs: DEFAULT_AUTO_LOCK_SECS,
        }
    }
}

impl AppSettings {
    fn file_path(dir: &Path) -> PathBuf {
        dir.join("app_settings.json")
    }

    pub(crate) fn load(dir: &Path) -> Self {
        let settings: Self = std::fs::read(Self::file_path(dir))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            auto_lock_secs: settings
                .auto_lock_secs
                .clamp(MIN_AUTO_LOCK_SECS, MAX_AUTO_LOCK_SECS),
        }
    }

    pub(crate) fn save(&self, dir: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|e| format!("Encode settings: {e}"))?;
        crate::seed::write_file_atomic(&Self::file_path(dir), &bytes)
    }
}

/// Persisted password-attempt state. Survives an app restart, so killing the
/// app does not reset the counter.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PasswordLimiter {
    failed: u32,
    /// Unix time (seconds) until which password checks are refused.
    locked_until: u64,
}

impl PasswordLimiter {
    pub(crate) fn file_path(dir: &Path) -> PathBuf {
        dir.join("auth_state.json")
    }

    pub(crate) fn load(dir: &Path) -> Self {
        std::fs::read(Self::file_path(dir))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save(&self, dir: &Path) {
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = crate::seed::write_file_atomic(&Self::file_path(dir), &bytes);
        }
    }

    /// `Err` with a user-facing message while a cooldown is running.
    pub(crate) fn check(&self, now: u64) -> Result<(), String> {
        if self.locked_until > now {
            return Err(format!(
                "Too many attempts. Wait {} seconds.",
                self.locked_until - now
            ));
        }
        Ok(())
    }

    /// Record a wrong password and return the message to show.
    pub(crate) fn record_failure(&mut self, now: u64) -> String {
        self.failed = self.failed.saturating_add(1);
        if self.failed >= MAX_PASSWORD_ATTEMPTS {
            let doublings = (self.failed - MAX_PASSWORD_ATTEMPTS).min(6);
            let cooldown = (FIRST_COOLDOWN_SECS << doublings).min(MAX_COOLDOWN_SECS);
            self.locked_until = now + cooldown;
            format!(
                "Wrong password. Too many attempts — locked for {} seconds.",
                cooldown
            )
        } else {
            format!(
                "Wrong password. {} attempts remaining.",
                MAX_PASSWORD_ATTEMPTS - self.failed
            )
        }
    }

    pub(crate) fn record_success(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn is_clean(&self) -> bool {
        self.failed == 0 && self.locked_until == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_four_mistakes_then_escalates() {
        let mut limiter = PasswordLimiter::default();
        let now = 1_000_000;
        for remaining in (1..MAX_PASSWORD_ATTEMPTS).rev() {
            assert!(limiter.check(now).is_ok());
            assert_eq!(
                format!("Wrong password. {remaining} attempts remaining."),
                limiter.record_failure(now)
            );
        }
        assert!(limiter
            .record_failure(now)
            .contains("locked for 60 seconds"));
        assert_eq!(
            Err("Too many attempts. Wait 60 seconds.".to_string()),
            limiter.check(now)
        );
        assert!(limiter.check(now + 60).is_ok());

        // Further failures double the cooldown, up to the cap.
        assert!(limiter.record_failure(now + 60).contains("120 seconds"));
        assert!(limiter.record_failure(now + 300).contains("240 seconds"));
        for _ in 0..10 {
            limiter.record_failure(now);
        }
        assert!(limiter.record_failure(now).contains("3600 seconds"));

        limiter.record_success();
        assert!(limiter.is_clean());
        assert!(limiter.check(now).is_ok());
    }

    #[test]
    fn limiter_messages_match_the_unlock_screen_parser() {
        // UnlockScreen extracts the cooldown with
        // /(?:locked for|Wait)\s+(\d+)\s+second/i
        let mut limiter = PasswordLimiter::default();
        for _ in 0..MAX_PASSWORD_ATTEMPTS - 1 {
            limiter.record_failure(10);
        }
        let locked = limiter.record_failure(10);
        assert!(locked.contains("locked for 60 second"));
        assert!(limiter.check(10).unwrap_err().contains("Wait 60 second"));
    }

    #[test]
    fn limiter_state_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("npt-limiter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut limiter = PasswordLimiter::default();
        limiter.record_failure(5);
        limiter.save(&dir);
        assert_eq!(limiter, PasswordLimiter::load(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn app_settings_are_clamped_and_persisted() {
        let dir = std::env::temp_dir().join(format!("npt-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            DEFAULT_AUTO_LOCK_SECS,
            AppSettings::load(&dir).auto_lock_secs
        );
        AppSettings {
            auto_lock_secs: 300,
        }
        .save(&dir)
        .unwrap();
        assert_eq!(300, AppSettings::load(&dir).auto_lock_secs);
        AppSettings { auto_lock_secs: 5 }.save(&dir).unwrap();
        assert_eq!(MIN_AUTO_LOCK_SECS, AppSettings::load(&dir).auto_lock_secs);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn activity_clock_is_monotone_enough() {
        let a = activity_clock_secs();
        let b = activity_clock_secs();
        assert!(b >= a);
    }
}
