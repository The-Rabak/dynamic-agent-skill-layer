use chrono::{DateTime, Duration, Utc};

/// Default warning age (in days) for pending lifecycle artifacts.
pub const PENDING_DEFAULT_WARNING_AFTER_DAYS: i64 = 30;
/// Default expiry age (in days) for pending lifecycle artifacts.
pub const PENDING_DEFAULT_EXPIRY_AFTER_DAYS: i64 = 90;

/// Calculates the default warning timestamp for pending lifecycle artifacts.
pub fn pending_default_warning_at(created_at: DateTime<Utc>) -> DateTime<Utc> {
    created_at + Duration::days(PENDING_DEFAULT_WARNING_AFTER_DAYS)
}

/// Calculates the default expiry timestamp for pending lifecycle artifacts.
pub fn pending_default_expires_at(created_at: DateTime<Utc>) -> DateTime<Utc> {
    created_at + Duration::days(PENDING_DEFAULT_EXPIRY_AFTER_DAYS)
}
