use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;

use crate::config::CacheConfig;
use crate::error::{Error, Result};
use crate::expand;
use crate::model::{CacheWindow, CreateSeries, Instance, Override, Series, UpdateSeries};
use crate::store::Store;

/// The main public API for the hybrid recurring calendar engine.
///
/// Composes the Redis storage layer with pure RRULE expansion logic,
/// implementing the cache-hit/miss flow described in the hybrid technique.
pub struct Engine {
    store: Store,
    config: CacheConfig,
}

impl Engine {
    pub fn new(conn: redis::aio::MultiplexedConnection, config: CacheConfig) -> Self {
        let store = Store::new(conn, config.key_prefix.as_deref());
        Self { store, config }
    }

    // === QUERIES ===

    /// Query all events for a calendar within a UTC time range.
    ///
    /// Implements the full hybrid cache-hit/miss flow:
    /// 1. Identify candidate series for the calendar
    /// 2. For each series, ensure cache covers the requested range (expand on miss)
    /// 3. Single ZRANGEBYSCORE query across all series
    /// 4. Return materialized instances sorted by start time
    pub async fn query_events(
        &self,
        calendar_id: &Uuid,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Instance>> {
        let series_ids = self.store.get_calendar_series_ids(calendar_id).await?;

        // Ensure cache coverage for each series
        for sid in &series_ids {
            self.ensure_cache_coverage(sid, range_start, range_end)
                .await?;
        }

        // Single sorted-set range query across all series
        let instance_ids = self
            .store
            .query_instance_ids(calendar_id, range_start, range_end)
            .await?;

        let mut instances = self.store.get_instances(&instance_ids).await?;

        // Filter out cancelled instances
        instances.retain(|i| !i.is_cancelled);

        // Sort by start time
        instances.sort_by_key(|i| i.start_utc);

        Ok(instances)
    }

    /// Get a single instance by expanding on-the-fly.
    pub async fn get_instance(
        &self,
        series_id: &Uuid,
        recurrence_id: &NaiveDateTime,
    ) -> Result<Option<Instance>> {
        let series = self
            .store
            .get_series(series_id)
            .await?
            .ok_or(Error::SeriesNotFound(*series_id))?;

        let overrides = self.store.get_overrides_for_series(series_id).await?;
        let ovr = overrides.iter().find(|o| o.recurrence_id == *recurrence_id);

        // Build instance directly without cache
        let instances = expand::expand_series(
            &series,
            ovr.map(std::slice::from_ref).unwrap_or(&[]),
            DateTime::<Utc>::MIN_UTC,
            DateTime::<Utc>::MAX_UTC,
        )?;

        Ok(instances
            .into_iter()
            .find(|i| i.recurrence_id == *recurrence_id))
    }

    /// Get a series by ID.
    pub async fn get_series(&self, series_id: &Uuid) -> Result<Option<Series>> {
        self.store.get_series(series_id).await
    }

    // === WRITES ===

    /// Create a new recurring (or one-off) series. No eager expansion.
    pub async fn create_series(&self, input: CreateSeries) -> Result<Series> {
        let now = Utc::now();
        let series = Series {
            id: Uuid::new_v4(),
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
        };
        self.store.put_series(&series).await?;
        Ok(series)
    }

    /// Update a series. Checks optimistic concurrency, bumps version, invalidates cache.
    pub async fn update_series(
        &self,
        series_id: &Uuid,
        expected_version: u64,
        update: UpdateSeries,
    ) -> Result<Series> {
        let mut series = self
            .store
            .get_series(series_id)
            .await?
            .ok_or(Error::SeriesNotFound(*series_id))?;

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
        series.updated_at = Utc::now();

        self.store.put_series(&series).await?;
        // Invalidate cache -- lazy re-expansion on next query
        self.store
            .invalidate_cache_for_series(series_id, &series.calendar_id)
            .await?;

        Ok(series)
    }

    /// Delete a series and all its overrides and cached instances.
    pub async fn delete_series(&self, series_id: &Uuid) -> Result<()> {
        let series = self
            .store
            .get_series(series_id)
            .await?
            .ok_or(Error::SeriesNotFound(*series_id))?;

        self.store
            .invalidate_cache_for_series(series_id, &series.calendar_id)
            .await?;
        self.store.delete_overrides_for_series(series_id).await?;
        self.store
            .delete_series(series_id, &series.calendar_id)
            .await?;

        Ok(())
    }

    /// Create or update a per-instance override. Invalidates cache for the series.
    pub async fn edit_instance(&self, ovr: Override) -> Result<()> {
        let series = self
            .store
            .get_series(&ovr.series_id)
            .await?
            .ok_or(Error::SeriesNotFound(ovr.series_id))?;

        self.store.put_override(&ovr).await?;
        self.store
            .invalidate_cache_for_series(&ovr.series_id, &series.calendar_id)
            .await?;
        Ok(())
    }

    /// Cancel a single instance (shorthand for creating a cancellation override).
    pub async fn cancel_instance(
        &self,
        series_id: &Uuid,
        recurrence_id: &NaiveDateTime,
    ) -> Result<()> {
        let ovr = Override {
            series_id: *series_id,
            recurrence_id: *recurrence_id,
            is_cancelled: true,
            dtstart_local: None,
            duration_secs: None,
            title: None,
            payload: None,
        };
        self.edit_instance(ovr).await
    }

    /// Split a series at a given recurrence point ("this and future" edit).
    ///
    /// The old series gets an `until_utc` just before the split point.
    /// A new series is created starting at the split point.
    /// This avoids "exception explosion" per Google Calendar guidance.
    pub async fn split_series(
        &self,
        series_id: &Uuid,
        split_at: &NaiveDateTime,
        new_series_data: CreateSeries,
    ) -> Result<(Series, Series)> {
        let mut old_series = self
            .store
            .get_series(series_id)
            .await?
            .ok_or(Error::SeriesNotFound(*series_id))?;

        // Compute UNTIL: the UTC time of the split point (exclusive boundary)
        let until_utc = old_series
            .tzid
            .from_local_datetime(split_at)
            .earliest()
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| split_at.and_utc());

        old_series.until_utc = Some(until_utc);
        old_series.version += 1;
        old_series.updated_at = Utc::now();

        self.store.put_series(&old_series).await?;
        self.store
            .invalidate_cache_for_series(series_id, &old_series.calendar_id)
            .await?;

        let new_series = self.create_series(new_series_data).await?;

        Ok((old_series, new_series))
    }

    /// Force-expand a series for a given range (useful for background cache warming).
    pub async fn warm_cache(
        &self,
        series_id: &Uuid,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<()> {
        self.ensure_cache_coverage(series_id, range_start, range_end)
            .await
    }

    // === INTERNAL ===

    async fn ensure_cache_coverage(
        &self,
        series_id: &Uuid,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<()> {
        let existing_window = self.store.get_cache_window(series_id).await?;

        if let Some(ref window) = existing_window
            && window.covers(range_start, range_end)
        {
            return Ok(()); // Cache hit
        }

        // Cache miss: expand a generous window
        let now = Utc::now();
        let expand_start = range_start.min(now - self.config.lookbehind);
        let expand_end = range_end.max(now + self.config.lookahead);

        let series = match self.store.get_series(series_id).await? {
            Some(s) => s,
            None => return Ok(()), // Series deleted concurrently, skip
        };

        let overrides = self.store.get_overrides_for_series(series_id).await?;

        let instances = expand::expand_series(&series, &overrides, expand_start, expand_end)?;

        self.store
            .write_instances_to_cache(
                &series.calendar_id,
                series_id,
                &instances,
                self.config.instance_ttl,
            )
            .await?;

        // Update cache window (union with existing if any)
        let new_window = match existing_window {
            Some(w) => w.union(expand_start, expand_end),
            None => CacheWindow {
                start_utc: expand_start,
                end_utc: expand_end,
            },
        };
        self.store.put_cache_window(series_id, &new_window).await?;

        Ok(())
    }
}
