//! Integration tests for the pure free functions in `defernodate::pure`.

use chrono::{NaiveDate, TimeZone, Utc};
use defernodate::{apply_update, build_series, CreateSeries, Error, UpdateSeries};
use uuid::Uuid;

#[test]
fn build_series_sets_all_fields_from_input() {
    let id = Uuid::new_v4();
    let cal = Uuid::new_v4();
    let now = Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap();
    let input = CreateSeries {
        calendar_id: cal,
        title: "Test".into(),
        dtstart_local: NaiveDate::from_ymd_opt(2026, 4, 23)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap(),
        tzid: chrono_tz::America::New_York,
        duration_secs: 3600,
        rrule: Some("FREQ=DAILY".into()),
    };

    let s = build_series(input, id, now);

    assert_eq!(s.id, id);
    assert_eq!(s.calendar_id, cal);
    assert_eq!(s.title, "Test");
    assert_eq!(s.duration_secs, 3600);
    assert_eq!(s.rrule.as_deref(), Some("FREQ=DAILY"));
    assert_eq!(s.version, 1);
    assert_eq!(s.created_at, now);
    assert_eq!(s.updated_at, now);
    assert!(s.exdates.is_empty());
    assert!(s.until_utc.is_none());
}

#[test]
fn build_series_is_deterministic_given_id_and_now() {
    let id = Uuid::nil();
    let cal = Uuid::nil();
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let input = CreateSeries {
        calendar_id: cal,
        title: "Ping".into(),
        dtstart_local: NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        tzid: chrono_tz::UTC,
        duration_secs: 0,
        rrule: None,
    };
    let s1 = build_series(input.clone(), id, now);
    let s2 = build_series(input, id, now);
    assert_eq!(s1.id, s2.id);
    assert_eq!(s1.created_at, s2.created_at);
    assert_eq!(s1.version, s2.version);
}

fn sample_series() -> defernodate::Series {
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    build_series(
        CreateSeries {
            calendar_id: Uuid::nil(),
            title: "Original".into(),
            dtstart_local: NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            tzid: chrono_tz::UTC,
            duration_secs: 3600,
            rrule: Some("FREQ=DAILY".into()),
        },
        Uuid::nil(),
        now,
    )
}

#[test]
fn apply_update_bumps_version_and_updated_at() {
    let s = sample_series();
    let later = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let update = UpdateSeries {
        title: Some("Renamed".into()),
        ..Default::default()
    };
    let s2 = apply_update(s, 1, update, later).unwrap();
    assert_eq!(s2.title, "Renamed");
    assert_eq!(s2.version, 2);
    assert_eq!(s2.updated_at, later);
}

#[test]
fn apply_update_returns_version_conflict_when_mismatched() {
    let s = sample_series();
    let later = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let update = UpdateSeries::default();
    let err = apply_update(s, 99, update, later).unwrap_err();
    match err {
        Error::VersionConflict { expected, actual } => {
            assert_eq!(expected, 99);
            assert_eq!(actual, 1);
        }
        _ => panic!("expected VersionConflict, got {err:?}"),
    }
}

#[test]
fn apply_update_only_patches_some_fields() {
    let s = sample_series();
    let later = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let update = UpdateSeries {
        duration_secs: Some(7200),
        ..Default::default()
    };
    let s2 = apply_update(s, 1, update, later).unwrap();
    assert_eq!(s2.title, "Original");   // untouched
    assert_eq!(s2.duration_secs, 7200); // patched
    assert_eq!(s2.rrule.as_deref(), Some("FREQ=DAILY")); // untouched
}

#[test]
fn apply_update_replaces_exdates_when_provided() {
    let s = sample_series();
    let later = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let new_exdates = vec![NaiveDate::from_ymd_opt(2026, 1, 15)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap()];
    let update = UpdateSeries {
        exdates: Some(new_exdates.clone()),
        ..Default::default()
    };
    let s2 = apply_update(s, 1, update, later).unwrap();
    assert_eq!(s2.exdates, new_exdates);
}
