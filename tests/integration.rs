use chrono::{NaiveDate, TimeZone, Utc};
use defernodate::{CacheConfig, CreateSeries, Engine, Override, UpdateSeries};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;
use uuid::Uuid;

async fn setup() -> (Engine, testcontainers::ContainerAsync<Redis>) {
    let container = Redis::default().start().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let client = redis::Client::open(format!("redis://127.0.0.1:{port}")).unwrap();
    let conn = client.get_multiplexed_async_connection().await.unwrap();
    let engine = Engine::new(conn, CacheConfig::default());
    (engine, container)
}

fn dt(year: i32, month: u32, day: u32, hour: u32, min: u32) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_hms_opt(hour, min, 0)
        .unwrap()
}

fn utc(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
}

#[tokio::test]
async fn test_create_and_query_oneoff() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Team lunch".into(),
            dtstart_local: dt(2026, 4, 15, 12, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: None,
        })
        .await
        .unwrap();

    assert_eq!(series.version, 1);

    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 5, 1))
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "Team lunch");
    assert_eq!(events[0].series_id, series.id);
}

#[tokio::test]
async fn test_weekly_recurring_query() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Standup".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 1800,
            rrule: Some("FREQ=WEEKLY;BYDAY=MO,WE,FR".into()),
        })
        .await
        .unwrap();

    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 15))
        .await
        .unwrap();

    // April 1 (Wed), 3 (Fri), 6 (Mon), 8 (Wed), 10 (Fri), 13 (Mon)
    assert_eq!(events.len(), 6);

    // Verify sorted by start_utc
    for pair in events.windows(2) {
        assert!(pair[0].start_utc <= pair[1].start_utc);
    }
}

#[tokio::test]
async fn test_cache_hit_on_second_query() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Daily".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::US::Eastern,
            duration_secs: 3600,
            rrule: Some("FREQ=DAILY".into()),
        })
        .await
        .unwrap();

    // First query: cache miss, triggers expansion
    let r1 = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 8))
        .await
        .unwrap();

    // Second query within same range: cache hit
    let r2 = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 8))
        .await
        .unwrap();

    assert_eq!(r1.len(), r2.len());
    assert_eq!(r1.len(), 7);

    // Verify series can be retrieved
    let fetched = engine.get_series(&series.id).await.unwrap().unwrap();
    assert_eq!(fetched.title, "Daily");
}

#[tokio::test]
async fn test_update_series_invalidates_cache() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Weekly".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: Some("FREQ=WEEKLY;BYDAY=WE".into()),
        })
        .await
        .unwrap();

    // Initial query
    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 30))
        .await
        .unwrap();
    let count_before = events.len();

    // Change to MO,WE,FR (more frequent)
    let updated = engine
        .update_series(
            &series.id,
            1,
            UpdateSeries {
                rrule: Some("FREQ=WEEKLY;BYDAY=MO,WE,FR".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.version, 2);

    // Query again -- should reflect new rule
    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 30))
        .await
        .unwrap();

    assert!(events.len() > count_before);
}

#[tokio::test]
async fn test_optimistic_concurrency() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Event".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: None,
        })
        .await
        .unwrap();

    // Update with wrong version
    let result = engine
        .update_series(
            &series.id,
            999, // wrong version
            UpdateSeries {
                title: Some("Changed".into()),
                ..Default::default()
            },
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        defernodate::Error::VersionConflict { expected, actual } => {
            assert_eq!(expected, 999);
            assert_eq!(actual, 1);
        }
        other => panic!("Expected VersionConflict, got: {other}"),
    }
}

#[tokio::test]
async fn test_override_single_instance() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Standup".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 1800,
            rrule: Some("FREQ=WEEKLY;BYDAY=WE".into()),
        })
        .await
        .unwrap();

    // Move April 8 standup to 10am
    engine
        .edit_instance(Override {
            series_id: series.id,
            recurrence_id: dt(2026, 4, 8, 9, 0),
            is_cancelled: false,
            dtstart_local: Some(dt(2026, 4, 8, 10, 0)),
            duration_secs: None,
            title: Some("Moved standup".into()),
            labels: None,
            description: None,
            payload: None,
        })
        .await
        .unwrap();

    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 15))
        .await
        .unwrap();

    let moved = events.iter().find(|e| e.is_override).unwrap();
    assert_eq!(moved.title, "Moved standup");
}

#[tokio::test]
async fn test_cancel_instance() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Standup".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 1800,
            rrule: Some("FREQ=WEEKLY;BYDAY=WE".into()),
        })
        .await
        .unwrap();

    // Cancel April 8
    engine
        .cancel_instance(&series.id, &dt(2026, 4, 8, 9, 0))
        .await
        .unwrap();

    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 15))
        .await
        .unwrap();

    // Should have April 1 but not April 8 (cancelled instances are filtered)
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].start_local, dt(2026, 4, 1, 9, 0));
}

#[tokio::test]
async fn test_split_series() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Weekly review".into(),
            dtstart_local: dt(2026, 4, 1, 14, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: Some("FREQ=WEEKLY;BYDAY=WE".into()),
        })
        .await
        .unwrap();

    // Split at April 15: old series stops, new series starts with different time
    let (old_series, new_series) = engine
        .split_series(
            &series.id,
            &dt(2026, 4, 15, 14, 0),
            CreateSeries {
                calendar_id: cal_id,
                title: "Weekly review (updated)".into(),
                dtstart_local: dt(2026, 4, 15, 15, 0), // moved to 3pm
                tzid: chrono_tz::America::New_York,
                duration_secs: 3600,
                rrule: Some("FREQ=WEEKLY;BYDAY=WE".into()),
            },
        )
        .await
        .unwrap();

    assert!(old_series.until_utc.is_some());
    assert_eq!(new_series.title, "Weekly review (updated)");

    // Query the full range
    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 30))
        .await
        .unwrap();

    // Old series: April 1, 8 (before split)
    // New series: April 15, 22, 29 (after split at 3pm)
    let old_events: Vec<_> = events
        .iter()
        .filter(|e| e.series_id == old_series.id)
        .collect();
    let new_events: Vec<_> = events
        .iter()
        .filter(|e| e.series_id == new_series.id)
        .collect();
    assert_eq!(old_events.len(), 2);
    assert_eq!(new_events.len(), 3);
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn test_delete_series() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Temp".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: Some("FREQ=DAILY".into()),
        })
        .await
        .unwrap();

    // Populate cache
    engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 8))
        .await
        .unwrap();

    // Delete
    engine.delete_series(&series.id).await.unwrap();

    // Verify gone
    let fetched = engine.get_series(&series.id).await.unwrap();
    assert!(fetched.is_none());

    // Verify no events
    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 8))
        .await
        .unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_multiple_series_same_calendar() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Morning standup".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 900,
            rrule: Some("FREQ=WEEKLY;BYDAY=MO".into()),
        })
        .await
        .unwrap();

    engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Afternoon review".into(),
            dtstart_local: dt(2026, 4, 1, 15, 0),
            tzid: chrono_tz::America::New_York,
            duration_secs: 3600,
            rrule: Some("FREQ=WEEKLY;BYDAY=FR".into()),
        })
        .await
        .unwrap();

    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 15))
        .await
        .unwrap();

    // Mon Apr 6, Mon Apr 13 (standup) + Fri Apr 3, Fri Apr 10 (review) = 4
    assert_eq!(events.len(), 4);

    // Should be sorted by start_utc
    for pair in events.windows(2) {
        assert!(pair[0].start_utc <= pair[1].start_utc);
    }
}

#[tokio::test]
async fn test_warm_cache() {
    let (engine, _container) = setup().await;
    let cal_id = Uuid::new_v4();

    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Daily".into(),
            dtstart_local: dt(2026, 4, 1, 9, 0),
            tzid: chrono_tz::US::Eastern,
            duration_secs: 3600,
            rrule: Some("FREQ=DAILY".into()),
        })
        .await
        .unwrap();

    // Warm cache explicitly
    engine
        .warm_cache(&series.id, utc(2026, 4, 1), utc(2026, 4, 8))
        .await
        .unwrap();

    // Query should be a cache hit
    let events = engine
        .query_events(&cal_id, utc(2026, 4, 1), utc(2026, 4, 8))
        .await
        .unwrap();

    assert_eq!(events.len(), 7);
}
