# Changelog

## 0.2.0 — 2026-04-23

**Breaking:** Pure calculation library. No Redis, no async, no runtime.

### Removed

- `Engine` struct — all methods moved to free functions.
- `Store` struct — storage is now the caller's responsibility.
- `Keys` struct — callers own their key schema.
- `redis`, `tokio`, `testcontainers` dependencies.
- `CacheConfig::key_prefix` field (key schema is caller-owned now).
- `Error::Redis` variant.

### Added

- `expand_series(series, overrides, range_start, range_end) -> Result<Vec<Instance>>` — pure RRULE expansion with overrides applied.
- `get_instance(series, overrides, recurrence_id) -> Option<Instance>` — single-instance lookup.
- `build_series(input, id, now) -> Series` — construct a new `Series`. Caller supplies `id` + `now`.
- `apply_update(series, expected_version, update, now) -> Result<Series>` — optimistic-concurrency patching.
- `split_series(old, split_at, new, new_id, now) -> (Series, Series)` — "this and future" edit primitive.

### Migration from 0.1.x

Storage is now the caller's job. Replace `Engine::create_series(input)` with:

```rust
let series = defernodate::build_series(input, Uuid::new_v4(), Utc::now());
your_storage.put_series(&series).await?;
```

Replace `Engine::query_events(cal, start, end)` with your own
cache-hit/miss flow that calls `expand_series` on cache miss and writes
the result to your store.

## 0.1.2

Last release with embedded Redis storage.
