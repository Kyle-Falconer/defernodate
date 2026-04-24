//! Integration tests for the pure free functions in `defernodate::pure`.

use chrono::{NaiveDate, TimeZone, Utc};
use defernodate::{apply_update, build_series, get_instance, split_series, CreateSeries, Error, Override, UpdateSeries};
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

#[test]
fn split_series_sets_until_on_old_and_creates_new() {
    let s = sample_series();
    let old_id = s.id;
    let split_at = NaiveDate::from_ymd_opt(2026, 2, 1)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let new_id = Uuid::new_v4();
    let now = Utc.with_ymd_and_hms(2026, 1, 15, 12, 0, 0).unwrap();
    let new_input = CreateSeries {
        calendar_id: s.calendar_id,
        title: "Renamed series".into(),
        dtstart_local: split_at,
        tzid: s.tzid,
        duration_secs: s.duration_secs,
        rrule: s.rrule.clone(),
    };
    let (old, new) = split_series(s, &split_at, new_input, new_id, now);

    assert_eq!(old.id, old_id);
    assert!(old.until_utc.is_some());
    assert_eq!(old.version, 2);
    assert_eq!(old.updated_at, now);

    assert_eq!(new.id, new_id);
    assert_eq!(new.title, "Renamed series");
    assert_eq!(new.version, 1);
    assert_eq!(new.created_at, now);
}

#[test]
fn split_series_until_respects_tzid() {
    // DST-aware tz: NY. Split at 2026-03-09T09:00 local (after spring-forward).
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let s = build_series(
        CreateSeries {
            calendar_id: Uuid::nil(),
            title: "NY".into(),
            dtstart_local: NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            tzid: chrono_tz::America::New_York,
            duration_secs: 0,
            rrule: Some("FREQ=DAILY".into()),
        },
        Uuid::nil(),
        now,
    );
    let split_at = NaiveDate::from_ymd_opt(2026, 3, 9)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let (old, _new) = split_series(
        s,
        &split_at,
        CreateSeries {
            calendar_id: Uuid::nil(),
            title: "NY2".into(),
            dtstart_local: split_at,
            tzid: chrono_tz::America::New_York,
            duration_secs: 0,
            rrule: Some("FREQ=DAILY".into()),
        },
        Uuid::new_v4(),
        now,
    );
    // 09:00 NY on 2026-03-09 is EDT (UTC-4) = 13:00 UTC
    let until = old.until_utc.unwrap();
    assert_eq!(until.format("%Y-%m-%dT%H:%M:%S").to_string(), "2026-03-09T13:00:00");
}

#[test]
fn get_instance_returns_some_for_in_series_recurrence() {
    let s = sample_series();
    let rec = NaiveDate::from_ymd_opt(2026, 1, 5)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let inst = get_instance(&s, &[], &rec).expect("instance exists");
    assert_eq!(inst.recurrence_id, rec);
    assert_eq!(inst.title, "Original");
    assert!(!inst.is_cancelled);
}

#[test]
fn get_instance_returns_none_for_cancelled_override() {
    let s = sample_series();
    let rec = NaiveDate::from_ymd_opt(2026, 1, 5)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let ovr = Override {
        series_id: s.id,
        recurrence_id: rec,
        is_cancelled: true,
        dtstart_local: None,
        duration_secs: None,
        title: None,
        labels: None,
        description: None,
        payload: None,
    };
    let inst = get_instance(&s, &[ovr], &rec);
    assert!(inst.is_none());
}

#[test]
fn get_instance_returns_none_for_non_occurring_recurrence() {
    // Weekly on Monday; ask for a Sunday.
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let s = build_series(
        CreateSeries {
            calendar_id: Uuid::nil(),
            title: "Mondays only".into(),
            dtstart_local: NaiveDate::from_ymd_opt(2026, 1, 5)  // 2026-01-05 is Mon
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            tzid: chrono_tz::UTC,
            duration_secs: 0,
            rrule: Some("FREQ=WEEKLY;BYDAY=MO".into()),
        },
        Uuid::nil(),
        now,
    );
    let sunday = NaiveDate::from_ymd_opt(2026, 1, 11)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let inst = get_instance(&s, &[], &sunday);
    assert!(inst.is_none());
}
