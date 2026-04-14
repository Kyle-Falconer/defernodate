# defernodate

A Rust library crate implementing the **hybrid master + instance cache** pattern for recurring calendar events, backed by Redis.

This is the same architectural pattern used by Apple's CalendarServer: store canonical RRULE rules as the source of truth, maintain a bounded materialized instance cache in Redis sorted sets for fast time-range queries, and expand lazily on cache miss.

## Features

- **RFC 5545 recurrence** — RRULE, EXDATE, and per-instance overrides via the `rrule` crate
- **Hybrid caching** — lazy expansion into Redis sorted sets; queries are O(log n + k) cache hits
- **DST-correct** — local DTSTART + IANA timezone; expands in local time, indexes in UTC
- **Optimistic concurrency** — version field on series prevents silent overwrites
- **Split series** — "this and future" edits split the series instead of creating override explosions
- **Async** — built on `tokio` and `redis-rs` async

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
defernodate = { path = "." }  # or git/crates.io once published
```

```rust
use chrono::{NaiveDate, TimeZone, Utc};
use defernodate::{CacheConfig, CreateSeries, Engine};
use uuid::Uuid;

#[tokio::main]
async fn main() -> defernodate::Result<()> {
    let client = redis::Client::open("redis://127.0.0.1:6379")?;
    let conn = client.get_multiplexed_async_connection().await?;
    let engine = Engine::new(conn, CacheConfig::default());

    let cal_id = Uuid::new_v4();

    // Create a recurring event: every Mon/Wed/Fri at 9am Eastern
    let series = engine
        .create_series(CreateSeries {
            calendar_id: cal_id,
            title: "Standup".into(),
            dtstart_local: NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            tzid: chrono_tz::America::New_York,
            duration_secs: 1800,
            rrule: Some("FREQ=WEEKLY;BYDAY=MO,WE,FR".into()),
        })
        .await?;

    // Query a two-week window (expands on first call, cached after)
    let events = engine
        .query_events(
            &cal_id,
            Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap(),
        )
        .await?;

    println!("Got {} events for series {}", events.len(), series.id);
    Ok(())
}
```

## API overview

All operations go through the `Engine` struct:

| Method | Description |
|---|---|
| `create_series` | Create a recurring or one-off event |
| `query_events` | Time-range query with automatic cache expansion |
| `get_series` | Fetch a series by ID |
| `get_instance` | Fetch a single instance by series + recurrence ID |
| `update_series` | Update series rule/metadata (optimistic concurrency) |
| `delete_series` | Remove series and all cached data |
| `edit_instance` | Create/update a per-instance override |
| `cancel_instance` | Cancel a single occurrence |
| `split_series` | "This and future" edit — splits into two series |
| `warm_cache` | Pre-populate cache for a given range |

## Redis data model

| Key | Type | Purpose |
|---|---|---|
| `series:{id}` | String (JSON) | Canonical RRULE, DTSTART, EXDATE, version |
| `override:{series_id}:{ts}` | String (JSON) | Per-instance exception |
| `instances:{calendar_id}` | Sorted Set | Time-range index (score = UTC timestamp) |
| `instance:{instance_id}` | String (JSON) | Materialized instance payload |
| `cache_window:{series_id}` | String (JSON) | Expanded range tracking |
| `calendar_series:{calendar_id}` | Set | Series belonging to a calendar |

No Redis modules required — works with vanilla Redis.

## Configuration

```rust
use defernodate::CacheConfig;

let config = CacheConfig {
    lookbehind: chrono::Duration::days(30),   // how far back to expand on miss
    lookahead: chrono::Duration::days(180),   // how far forward to expand on miss
    instance_ttl: None,                       // optional TTL for cached instances
    key_prefix: Some("myapp".into()),         // namespace keys in shared Redis
};
```

## Testing

Unit tests (no Docker needed):

```sh
cargo test --lib
```

Integration tests (requires Docker):

```sh
cargo test --test integration -- --test-threads=1
```

All tests:

```sh
cargo test -- --test-threads=1
```

## Design references

The implementation is based on the techniques described in:

- *Techniques and Algorithms for Storing and Retrieving Recurring Calendar Events in Multi-User Systems* — covers rule-native, full materialization, and hybrid patterns
- Apple CalendarServer's `TIME_RANGE` cache with `RECURRANCE_MIN`/`RECURRANCE_MAX` tracking
- Google Calendar API guidance on avoiding exception explosions via series splitting

## License

MIT
