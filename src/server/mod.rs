#[cfg(feature = "ssr")]
pub mod attachments;
#[cfg(feature = "ssr")]
pub mod config;
#[cfg(feature = "ssr")]
pub mod db;
#[cfg(feature = "ssr")]
pub mod state;
#[cfg(feature = "ssr")]
pub mod storage;

/// Current UNIX time in epoch milliseconds (D-03). Single source of truth for
/// timestamp generation (WR-03). Surfaces a clock error to the caller rather
/// than silently writing `0` when the system clock predates the epoch.
#[cfg(feature = "ssr")]
pub fn now_millis() -> Result<i64, std::time::SystemTimeError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64)
}
