//! Pure, synchronous operations on Series / Override / Instance values.
//! I/O and non-determinism are the caller's responsibility: ids and
//! timestamps are passed in.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::expand;
use crate::model::{CreateSeries, Instance, Override, Series, UpdateSeries};

/// Get a single instance at a recurrence point, or `None` if the
/// series does not occur there (or the instance is cancelled).
pub fn get_instance(
    series: &Series,
    overrides: &[Override],
    recurrence_id: &NaiveDateTime,
) -> Option<Instance> {
    let ovr = overrides.iter().find(|o| &o.recurrence_id == recurrence_id);
    let instances = expand::expand_series(
        series,
        ovr.map(std::slice::from_ref).unwrap_or(&[]),
        DateTime::<Utc>::MIN_UTC,
        DateTime::<Utc>::MAX_UTC,
    )
    .ok()?;
    instances
        .into_iter()
        .find(|i| &i.recurrence_id == recurrence_id && !i.is_cancelled)
}

/// Build a new `Series` record from caller-supplied inputs.
/// Pure: id and `now` are passed in rather than generated internally.
pub fn build_series(input: CreateSeries, id: Uuid, now: DateTime<Utc>) -> Series {
    Series {
        id,
        calendar_id: input.calendar_id,
        title: input.title,
        dtstart_local: input.dtstart_local,
        tzid: input.tzid,
        duration_secs: input.duration_secs,
        rrule: input.rrule,
        exdates: vec![],
        until_utc: None,
        version: 1,
        created_at: now,
        updated_at: now,
    }
}

/// Apply an `UpdateSeries` patch to a `Series`. Bumps version and
/// stamps `updated_at`. Returns `Err(VersionConflict)` if
/// `expected_version` doesn't match `series.version`.
pub fn apply_update(
    mut series: Series,
    expected_version: u64,
    update: UpdateSeries,
    now: DateTime<Utc>,
) -> Result<Series> {
    if series.version != expected_version {
        return Err(Error::VersionConflict {
            expected: expected_version,
            actual: series.version,
        });
    }
    if let Some(title) = update.title {
        series.title = title;
    }
    if let Some(rrule) = update.rrule {
        series.rrule = Some(rrule);
    }
    if let Some(dtstart) = update.dtstart_local {
        series.dtstart_local = dtstart;
    }
    if let Some(tzid) = update.tzid {
        series.tzid = tzid;
    }
    if let Some(dur) = update.duration_secs {
        series.duration_secs = dur;
    }
    if let Some(exdates) = update.exdates {
        series.exdates = exdates;
    }
    series.version += 1;
    series.updated_at = now;
    Ok(series)
}

/// Compute the "this and future" split: the old series gets `until_utc`
/// set just before the split point; a new series starts at the split
/// point. Returns `(old_with_until, new_series)`.
pub fn split_series(
    mut old: Series,
    split_at: &NaiveDateTime,
    new: CreateSeries,
    new_id: Uuid,
    now: DateTime<Utc>,
) -> (Series, Series) {
    let until_utc = old
        .tzid
        .from_local_datetime(split_at)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| split_at.and_utc());

    old.until_utc = Some(until_utc);
    old.version += 1;
    old.updated_at = now;

    let new_series = build_series(new, new_id, now);
    (old, new_series)
}

// Placeholder: avoid "unused" warnings before tests are added.
#[cfg(test)]
mod tests {
    // Real tests live in Tasks 1.3, 1.4, 1.5, 1.6 (in tests/pure_ops.rs).
}
