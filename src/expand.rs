use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rrule::RRuleSet;

use crate::error::{Error, Result};
use crate::model::{Instance, Override, Series};

/// Build the RRULE set string that the `rrule` crate can parse.
fn build_rruleset_string(series: &Series) -> String {
    let tz_name = series.tzid.name();
    let dtstart_str = series.dtstart_local.format("%Y%m%dT%H%M%S");
    let mut lines = vec![format!("DTSTART;TZID={tz_name}:{dtstart_str}")];

    if let Some(ref rrule) = series.rrule {
        lines.push(format!("RRULE:{rrule}"));
    }

    for exdate in &series.exdates {
        let ex_str = exdate.format("%Y%m%dT%H%M%S");
        lines.push(format!("EXDATE;TZID={tz_name}:{ex_str}"));
    }

    lines.join("\n")
}

/// Generate a deterministic instance ID from series ID and recurrence local time.
fn instance_id(series: &Series, recurrence_local: &NaiveDateTime) -> String {
    format!("{}:{}", series.id, recurrence_local.and_utc().timestamp())
}

/// Convert `chrono_tz::Tz` to `rrule::Tz`.
fn to_rrule_tz(tz: chrono_tz::Tz) -> rrule::Tz {
    rrule::Tz::from(tz)
}

/// Expand a series within a bounded UTC range, applying overrides.
///
/// This is the core computation that the cache exists to amortize.
/// All RRULE expansion happens in local time (per RFC 5545), then
/// results are converted to UTC for indexing.
pub fn expand_series(
    series: &Series,
    overrides: &[Override],
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
) -> Result<Vec<Instance>> {
    // For one-off (non-recurring) events, just check if they fall in range.
    if series.rrule.is_none() {
        return expand_oneoff(series, overrides, range_start, range_end);
    }

    let rrule_str = build_rruleset_string(series);
    let rrule_set: RRuleSet = rrule_str
        .parse()
        .map_err(|e: rrule::RRuleError| Error::RRule(e.to_string()))?;

    // Convert UTC range to rrule's DateTime<Tz> for the .after()/.before() bounds.
    let rrule_tz = to_rrule_tz(series.tzid);
    let start_local = range_start.with_timezone(&rrule_tz);
    let end_local = range_end.with_timezone(&rrule_tz);

    let occurrences = rrule_set
        .after(start_local)
        .before(end_local)
        .all(u16::MAX)
        .dates;

    // Index overrides by recurrence_id for quick lookup.
    let overrides_map: std::collections::HashMap<NaiveDateTime, &Override> = overrides
        .iter()
        .map(|o| (o.recurrence_id, o))
        .collect();

    let mut instances = Vec::with_capacity(occurrences.len());

    for occ in occurrences {
        let local = occ.naive_local();

        // If series was split with UNTIL, skip occurrences past the boundary.
        if let Some(until) = series.until_utc {
            let occ_utc = occ.with_timezone(&Utc);
            if occ_utc >= until {
                continue;
            }
        }

        let inst = if let Some(ovr) = overrides_map.get(&local) {
            build_override_instance(series, ovr)
        } else {
            build_instance(series, local)
        };

        instances.push(inst);
    }

    Ok(instances)
}

fn expand_oneoff(
    series: &Series,
    overrides: &[Override],
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
) -> Result<Vec<Instance>> {
    let ovr = overrides
        .iter()
        .find(|o| o.recurrence_id == series.dtstart_local);

    let inst = if let Some(ovr) = ovr {
        build_override_instance(series, ovr)
    } else {
        build_instance(series, series.dtstart_local)
    };

    if inst.is_cancelled {
        return Ok(vec![]);
    }

    if inst.start_utc >= range_start && inst.start_utc < range_end {
        Ok(vec![inst])
    } else {
        Ok(vec![])
    }
}

fn build_instance(series: &Series, local: NaiveDateTime) -> Instance {
    let start_utc = series
        .tzid
        .from_local_datetime(&local)
        .earliest()
        .unwrap_or_else(|| Utc.from_utc_datetime(&local).with_timezone(&series.tzid))
        .with_timezone(&Utc);

    let end_utc = start_utc + chrono::Duration::seconds(series.duration_secs);

    Instance {
        instance_id: instance_id(series, &local),
        series_id: series.id,
        calendar_id: series.calendar_id,
        recurrence_id: local,
        start_utc,
        end_utc,
        start_local: local,
        tzid: series.tzid,
        title: series.title.clone(),
        is_cancelled: false,
        is_override: false,
        payload: None,
    }
}

fn build_override_instance(series: &Series, ovr: &Override) -> Instance {
    let actual_local = ovr.dtstart_local.unwrap_or(ovr.recurrence_id);
    let actual_duration = ovr.duration_secs.unwrap_or(series.duration_secs);

    let start_utc = series
        .tzid
        .from_local_datetime(&actual_local)
        .earliest()
        .unwrap_or_else(|| Utc.from_utc_datetime(&actual_local).with_timezone(&series.tzid))
        .with_timezone(&Utc);

    let end_utc = start_utc + chrono::Duration::seconds(actual_duration);

    Instance {
        instance_id: instance_id(series, &ovr.recurrence_id),
        series_id: series.id,
        calendar_id: series.calendar_id,
        recurrence_id: ovr.recurrence_id,
        start_utc,
        end_utc,
        start_local: actual_local,
        tzid: series.tzid,
        title: ovr.title.clone().unwrap_or_else(|| series.title.clone()),
        is_cancelled: ovr.is_cancelled,
        is_override: true,
        payload: ovr.payload.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Timelike};
    use uuid::Uuid;

    fn make_series(rrule: Option<&str>) -> Series {
        Series {
            id: Uuid::nil(),
            calendar_id: Uuid::nil(),
            title: "Test".into(),
            dtstart_local: NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: rrule.map(String::from),
            exdates: vec![],
            until_utc: None,
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_oneoff_in_range() {
        let series = make_series(None);
        let start = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap();
        let result = expand_series(&series, &[], start, end).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Test");
    }

    #[test]
    fn test_oneoff_out_of_range() {
        let series = make_series(None);
        let start = Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let result = expand_series(&series, &[], start, end).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_weekly_expansion() {
        let series = make_series(Some("FREQ=WEEKLY;BYDAY=MO,WE,FR"));
        let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap();
        let result = expand_series(&series, &[], start, end).unwrap();
        // April 1 (Wed), 3 (Fri), 6 (Mon), 8 (Wed), 10 (Fri), 13 (Mon)
        assert_eq!(result.len(), 6);
    }

    #[test]
    fn test_exdate_exclusion() {
        let mut series = make_series(Some("FREQ=WEEKLY;BYDAY=WE"));
        // Exclude April 8
        series.exdates.push(
            NaiveDate::from_ymd_opt(2026, 4, 8)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
        );
        let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap();
        let result = expand_series(&series, &[], start, end).unwrap();
        // April 1, 15 (8 excluded)
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_override_reschedule() {
        let series = make_series(Some("FREQ=WEEKLY;BYDAY=WE"));
        let override_recurrence = NaiveDate::from_ymd_opt(2026, 4, 8)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let overrides = vec![Override {
            series_id: series.id,
            recurrence_id: override_recurrence,
            is_cancelled: false,
            dtstart_local: Some(
                NaiveDate::from_ymd_opt(2026, 4, 8)
                    .unwrap()
                    .and_hms_opt(10, 0, 0)
                    .unwrap(),
            ),
            duration_secs: None,
            title: Some("Moved standup".into()),
            payload: None,
        }];
        let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap();
        let result = expand_series(&series, &overrides, start, end).unwrap();
        let moved = result.iter().find(|i| i.is_override).unwrap();
        assert_eq!(moved.title, "Moved standup");
        assert_eq!(moved.start_local.hour(), 10);
    }

    #[test]
    fn test_override_cancel() {
        let series = make_series(Some("FREQ=WEEKLY;BYDAY=WE"));
        let override_recurrence = NaiveDate::from_ymd_opt(2026, 4, 8)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let overrides = vec![Override {
            series_id: series.id,
            recurrence_id: override_recurrence,
            is_cancelled: true,
            dtstart_local: None,
            duration_secs: None,
            title: None,
            payload: None,
        }];
        let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap();
        let result = expand_series(&series, &overrides, start, end).unwrap();
        let cancelled = result
            .iter()
            .find(|i| i.recurrence_id == override_recurrence)
            .unwrap();
        assert!(cancelled.is_cancelled);
    }

    #[test]
    fn test_until_utc_boundary() {
        let mut series = make_series(Some("FREQ=WEEKLY;BYDAY=WE"));
        // Split: only instances before April 10 UTC
        series.until_utc = Some(Utc.with_ymd_and_hms(2026, 4, 10, 0, 0, 0).unwrap());
        let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
        let result = expand_series(&series, &[], start, end).unwrap();
        // Only April 1 and April 8 (April 15, 22, 29 are past until)
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_instance_id_determinism() {
        let series = make_series(Some("FREQ=WEEKLY;BYDAY=WE"));
        let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap();
        let r1 = expand_series(&series, &[], start, end).unwrap();
        let r2 = expand_series(&series, &[], start, end).unwrap();
        let ids1: Vec<_> = r1.iter().map(|i| &i.instance_id).collect();
        let ids2: Vec<_> = r2.iter().map(|i| &i.instance_id).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn test_dst_spring_forward() {
        // March 8, 2026: US spring forward. 9am stays 9am local but UTC offset changes.
        let series = Series {
            dtstart_local: NaiveDate::from_ymd_opt(2026, 3, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            ..make_series(Some("FREQ=WEEKLY;BYDAY=SU"))
        };
        let start = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 3, 22, 0, 0, 0).unwrap();
        let result = expand_series(&series, &[], start, end).unwrap();
        // All instances should be at 9am local
        for inst in &result {
            assert_eq!(inst.start_local.hour(), 9);
        }
        // But UTC times should differ: before DST = 14:00 UTC (EST), after = 13:00 UTC (EDT)
        assert!(result.len() >= 2);
        let pre_dst = &result[0]; // March 1
        let post_dst = &result[1]; // March 8 (spring forward)
        assert_eq!(pre_dst.start_utc.hour(), 14); // EST = UTC-5
        assert_eq!(post_dst.start_utc.hour(), 13); // EDT = UTC-4
    }
}
