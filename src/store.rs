use chrono::{DateTime, NaiveDateTime, Utc};
use redis::AsyncCommands;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::keys::Keys;
use crate::model::{CacheWindow, Instance, Override, Series};

/// Async Redis storage layer. Wraps all Redis I/O with typed methods.
pub struct Store {
    conn: redis::aio::MultiplexedConnection,
    keys: Keys,
}

impl Store {
    pub fn new(conn: redis::aio::MultiplexedConnection, key_prefix: Option<&str>) -> Self {
        Self {
            conn,
            keys: Keys::new(key_prefix),
        }
    }

    // --- Series ---

    pub async fn get_series(&self, id: &Uuid) -> Result<Option<Series>> {
        let key = self.keys.series(id);
        let val: Option<String> = self.conn.clone().get(&key).await?;
        val.map(|v| serde_json::from_str(&v).map_err(Error::from))
            .transpose()
    }

    pub async fn put_series(&self, series: &Series) -> Result<()> {
        let key = self.keys.series(&series.id);
        let val = serde_json::to_string(series)?;
        self.conn.clone().set::<_, _, ()>(&key, &val).await?;
        // Add to calendar index
        let cal_key = self.keys.calendar_series(&series.calendar_id);
        self.conn
            .clone()
            .sadd::<_, _, ()>(&cal_key, series.id.to_string())
            .await?;
        Ok(())
    }

    pub async fn delete_series(&self, id: &Uuid, calendar_id: &Uuid) -> Result<()> {
        let mut pipe = redis::pipe();
        pipe.del(self.keys.series(id))
            .srem(
                self.keys.calendar_series(calendar_id),
                id.to_string(),
            )
            .del(self.keys.cache_window(id))
            .del(self.keys.series_overrides(id));
        pipe.query_async::<()>(&mut self.conn.clone()).await?;
        Ok(())
    }

    pub async fn get_calendar_series_ids(&self, calendar_id: &Uuid) -> Result<Vec<Uuid>> {
        let key = self.keys.calendar_series(calendar_id);
        let members: Vec<String> = self.conn.clone().smembers(&key).await?;
        Ok(members
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect())
    }

    // --- Overrides ---

    pub async fn get_override(
        &self,
        series_id: &Uuid,
        recurrence_id: &NaiveDateTime,
    ) -> Result<Option<Override>> {
        let key = self.keys.override_key(series_id, recurrence_id);
        let val: Option<String> = self.conn.clone().get(&key).await?;
        val.map(|v| serde_json::from_str(&v).map_err(Error::from))
            .transpose()
    }

    pub async fn put_override(&self, ovr: &Override) -> Result<()> {
        let key = self.keys.override_key(&ovr.series_id, &ovr.recurrence_id);
        let val = serde_json::to_string(ovr)?;
        let index_key = self.keys.series_overrides(&ovr.series_id);
        let recurrence_ts = ovr.recurrence_id.and_utc().timestamp().to_string();
        let mut pipe = redis::pipe();
        pipe.set(&key, &val).sadd(&index_key, &recurrence_ts);
        pipe.query_async::<()>(&mut self.conn.clone()).await?;
        Ok(())
    }

    pub async fn get_overrides_for_series(&self, series_id: &Uuid) -> Result<Vec<Override>> {
        let index_key = self.keys.series_overrides(series_id);
        let timestamps: Vec<String> = self.conn.clone().smembers(&index_key).await?;
        if timestamps.is_empty() {
            return Ok(vec![]);
        }

        let keys: Vec<String> = timestamps
            .iter()
            .filter_map(|ts| {
                let secs: i64 = ts.parse().ok()?;
                let dt = DateTime::from_timestamp(secs, 0)?.naive_utc();
                Some(self.keys.override_key(series_id, &dt))
            })
            .collect();

        if keys.is_empty() {
            return Ok(vec![]);
        }

        let vals: Vec<Option<String>> = self.conn.clone().mget(&keys).await?;
        let mut overrides = Vec::new();
        for val in vals.into_iter().flatten() {
            if let Ok(ovr) = serde_json::from_str(&val) {
                overrides.push(ovr);
            }
        }
        Ok(overrides)
    }

    pub async fn delete_overrides_for_series(&self, series_id: &Uuid) -> Result<()> {
        let index_key = self.keys.series_overrides(series_id);
        let timestamps: Vec<String> = self.conn.clone().smembers(&index_key).await?;

        if timestamps.is_empty() {
            return Ok(());
        }

        let mut pipe = redis::pipe();
        for ts in &timestamps {
            if let Ok(secs) = ts.parse::<i64>()
                && let Some(dt) = DateTime::from_timestamp(secs, 0)
            {
                pipe.del(self.keys.override_key(series_id, &dt.naive_utc()));
            }
        }
        pipe.del(&index_key);
        pipe.query_async::<()>(&mut self.conn.clone()).await?;
        Ok(())
    }

    // --- Cache Window ---

    pub async fn get_cache_window(&self, series_id: &Uuid) -> Result<Option<CacheWindow>> {
        let key = self.keys.cache_window(series_id);
        let val: Option<String> = self.conn.clone().get(&key).await?;
        val.map(|v| serde_json::from_str(&v).map_err(Error::from))
            .transpose()
    }

    pub async fn put_cache_window(
        &self,
        series_id: &Uuid,
        window: &CacheWindow,
    ) -> Result<()> {
        let key = self.keys.cache_window(series_id);
        let val = serde_json::to_string(window)?;
        self.conn.clone().set::<_, _, ()>(&key, &val).await?;
        Ok(())
    }

    pub async fn delete_cache_window(&self, series_id: &Uuid) -> Result<()> {
        let key = self.keys.cache_window(series_id);
        self.conn.clone().del::<_, ()>(&key).await?;
        Ok(())
    }

    // --- Instance Cache ---

    pub async fn query_instance_ids(
        &self,
        calendar_id: &Uuid,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let key = self.keys.instances(calendar_id);
        let start_score = start.timestamp() as f64;
        let end_score = end.timestamp() as f64;
        let ids: Vec<String> = self
            .conn
            .clone()
            .zrangebyscore(&key, start_score, end_score)
            .await?;
        Ok(ids)
    }

    pub async fn get_instances(&self, instance_ids: &[String]) -> Result<Vec<Instance>> {
        if instance_ids.is_empty() {
            return Ok(vec![]);
        }
        let keys: Vec<String> = instance_ids
            .iter()
            .map(|id| self.keys.instance(id))
            .collect();
        let vals: Vec<Option<String>> = self.conn.clone().mget(&keys).await?;
        let mut instances = Vec::new();
        for val in vals.into_iter().flatten() {
            if let Ok(inst) = serde_json::from_str(&val) {
                instances.push(inst);
            }
        }
        Ok(instances)
    }

    pub async fn write_instances_to_cache(
        &self,
        calendar_id: &Uuid,
        series_id: &Uuid,
        instances: &[Instance],
        ttl: Option<std::time::Duration>,
    ) -> Result<()> {
        if instances.is_empty() {
            return Ok(());
        }

        let zset_key = self.keys.instances(calendar_id);
        let series_inst_key = self.keys.series_instances(series_id);
        let mut pipe = redis::pipe();

        for inst in instances {
            let score = inst.start_utc.timestamp() as f64;
            let inst_key = self.keys.instance(&inst.instance_id);
            let val = serde_json::to_string(inst)?;

            pipe.zadd(&zset_key, &inst.instance_id, score);
            pipe.sadd(&series_inst_key, &inst.instance_id);

            if let Some(ttl) = ttl {
                pipe.set_ex(&inst_key, &val, ttl.as_secs());
            } else {
                pipe.set(&inst_key, &val);
            }
        }

        pipe.query_async::<()>(&mut self.conn.clone()).await?;
        Ok(())
    }

    pub async fn invalidate_cache_for_series(
        &self,
        series_id: &Uuid,
        calendar_id: &Uuid,
    ) -> Result<()> {
        let series_inst_key = self.keys.series_instances(series_id);
        let instance_ids: Vec<String> = self.conn.clone().smembers(&series_inst_key).await?;

        if instance_ids.is_empty() {
            self.conn
                .clone()
                .del::<_, ()>(&self.keys.cache_window(series_id))
                .await?;
            return Ok(());
        }

        let zset_key = self.keys.instances(calendar_id);
        let mut pipe = redis::pipe();

        for id in &instance_ids {
            pipe.zrem(&zset_key, id);
            pipe.del(self.keys.instance(id));
        }
        pipe.del(&series_inst_key);
        pipe.del(self.keys.cache_window(series_id));

        pipe.query_async::<()>(&mut self.conn.clone()).await?;
        Ok(())
    }
}
