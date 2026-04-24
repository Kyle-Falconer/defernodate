# defernodate

A pure, synchronous Rust library for RFC 5545 recurrence expansion
(RRULE + EXDATE + per-instance overrides). No I/O, no async runtime,
no storage — the caller supplies ids, timestamps, and persistence.

## What it does

- Parses `FREQ=WEEKLY;BYDAY=MO,WE,FR`-style RRULE strings.
- Expands a `Series` across a bounded UTC range, applying
  `Override`s and cancellations.
- Handles DST correctly: local DTSTART + IANA timezone; expansion
  happens in local time, instances are indexed in UTC.
- Supports "this and future" splits via `split_series`, avoiding
  exception-list explosion on long-running series.

## What it does NOT do

- Store anything. A `Series`, its `Override`s, and any instance
  cache are the caller's problem. The previous 0.1.x line embedded
  a Redis store; 0.2.0 removed it so consumers can pick their own
  storage (Redis, Postgres, in-memory, etc.).
- Generate ids or timestamps. Callers pass `Uuid::new_v4()` and
  `Utc::now()` in at the call site, keeping the library
  deterministic and fully testable with fixed inputs.
- Run async. The whole public surface is synchronous.

## Quick start

```toml
[dependencies]
defernodate = "0.2"
```

```rust
use chrono::{NaiveDate, TimeZone, Utc};
use defernodate::{build_series, expand_series, CreateSeries};
use uuid::Uuid;

fn main() -> defernodate::Result<()> {
    let series = build_series(
        CreateSeries {
            calendar_id: Uuid::new_v4(),
            title: "Standup".into(),
            dtstart_local: NaiveDate::from_ymd_opt(2026, 4, 6)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            tzid: chrono_tz::America::New_York,
            duration_secs: 900,
            rrule: Some("FREQ=WEEKLY;BYDAY=MO,WE,FR".into()),
        },
        Uuid::new_v4(),
        Utc::now(),
    );

    let start = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap();
    let instances = expand_series(&series, &[], start, end)?;

    for inst in instances {
        println!("{} at {}", inst.title, inst.start_utc);
    }
    Ok(())
}
```

## Public API

- `build_series(input, id, now) -> Series`
- `apply_update(series, expected_version, update, now) -> Result<Series>`
- `split_series(old, split_at, new, new_id, now) -> (Series, Series)`
- `expand_series(series, overrides, range_start, range_end) -> Result<Vec<Instance>>`
- `get_instance(series, overrides, recurrence_id) -> Option<Instance>`

See `CHANGELOG.md` for migration notes from 0.1.x.

## License

MIT.
