//! Integration tests for the pure free functions in `defernodate::pure`.

use chrono::{NaiveDate, TimeZone, Utc};
use defernodate::{build_series, CreateSeries};
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
