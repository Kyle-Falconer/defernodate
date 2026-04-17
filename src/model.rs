use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The canonical source-of-truth for a recurring (or one-off) event.
/// Stored at Redis key `series:{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: Uuid,
    pub calendar_id: Uuid,
    pub title: String,
    /// Local wall time (NOT UTC). Paired with `tzid` to preserve intent across DST.
    pub dtstart_local: NaiveDateTime,
    /// IANA timezone (e.g. "America/Los_Angeles").
    pub tzid: chrono_tz::Tz,
    pub duration_secs: i64,
    /// RFC 5545 RRULE string (e.g. "FREQ=WEEKLY;BYDAY=MO,WE,FR"). None for one-off events.
    pub rrule: Option<String>,
    /// Local times of excluded instances (EXDATE).
    pub exdates: Vec<NaiveDateTime>,
    /// Derived upper bound. Set when a series is split ("this and future" edit).
    pub until_utc: Option<DateTime<Utc>>,
    /// Optimistic concurrency version. Bumped on every update.
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A per-instance exception (override), keyed by recurrence_id (the original DTSTART).
/// Stored at Redis key `override:{series_id}:{recurrence_id_ts}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Override {
    pub series_id: Uuid,
    /// Original local start time of this instance (RFC 5545 RECURRENCE-ID).
    pub recurrence_id: NaiveDateTime,
    pub is_cancelled: bool,
    /// Rescheduled start time (if moved).
    pub dtstart_local: Option<NaiveDateTime>,
    /// Override duration (if changed).
    pub duration_secs: Option<i64>,
    pub title: Option<String>,
    /// Override labels for this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// Override description for this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Arbitrary extra fields (location, attendees, etc.).
    pub payload: Option<serde_json::Value>,
}

/// A materialized (cached) event instance, ready for query return.
/// Stored at Redis key `instance:{instance_id}`.
/// Indexed in sorted set `instances:{calendar_id}` with score = start_utc timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// Deterministic ID: "{series_id}:{recurrence_id_ts}"
    pub instance_id: String,
    pub series_id: Uuid,
    pub calendar_id: Uuid,
    /// Original local start of this recurrence.
    pub recurrence_id: NaiveDateTime,
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
    pub start_local: NaiveDateTime,
    pub tzid: chrono_tz::Tz,
    pub title: String,
    pub labels: Vec<String>,
    pub description: Option<String>,
    pub is_cancelled: bool,
    pub is_override: bool,
    pub payload: Option<serde_json::Value>,
}

/// Tracks what time range has been expanded and cached for a given series.
/// Stored at Redis key `cache_window:{series_id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheWindow {
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
}

/// Input for creating a new series.
#[derive(Debug, Clone)]
pub struct CreateSeries {
    pub calendar_id: Uuid,
    pub title: String,
    pub dtstart_local: NaiveDateTime,
    pub tzid: chrono_tz::Tz,
    pub duration_secs: i64,
    pub rrule: Option<String>,
}

/// Input for updating series fields. Only `Some` fields are applied.
#[derive(Debug, Clone, Default)]
pub struct UpdateSeries {
    pub title: Option<String>,
    pub rrule: Option<String>,
    pub dtstart_local: Option<NaiveDateTime>,
    pub tzid: Option<chrono_tz::Tz>,
    pub duration_secs: Option<i64>,
    pub exdates: Option<Vec<NaiveDateTime>>,
}

impl CacheWindow {
    /// Returns true if this window fully covers the requested range.
    pub fn covers(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        self.start_utc <= start && self.end_utc >= end
    }

    /// Returns the union of this window and another range.
    pub fn union(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            start_utc: self.start_utc.min(start),
            end_utc: self.end_utc.max(end),
        }
    }
}
