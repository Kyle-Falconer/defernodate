use chrono::NaiveDateTime;
use uuid::Uuid;

/// Centralized Redis key construction. Single source of truth for all key patterns.
pub struct Keys {
    prefix: String,
}

impl Keys {
    pub fn new(prefix: Option<&str>) -> Self {
        Self {
            prefix: prefix.map(|p| format!("{p}:")).unwrap_or_default(),
        }
    }

    pub fn series(&self, id: &Uuid) -> String {
        format!("{}series:{id}", self.prefix)
    }

    pub fn override_key(&self, series_id: &Uuid, recurrence_id: &NaiveDateTime) -> String {
        format!(
            "{}override:{series_id}:{}",
            self.prefix,
            recurrence_id.and_utc().timestamp()
        )
    }

    pub fn series_overrides(&self, series_id: &Uuid) -> String {
        format!("{}series_overrides:{series_id}", self.prefix)
    }

    pub fn instances(&self, calendar_id: &Uuid) -> String {
        format!("{}instances:{calendar_id}", self.prefix)
    }

    pub fn instance(&self, instance_id: &str) -> String {
        format!("{}instance:{instance_id}", self.prefix)
    }

    pub fn series_instances(&self, series_id: &Uuid) -> String {
        format!("{}series_instances:{series_id}", self.prefix)
    }

    pub fn cache_window(&self, series_id: &Uuid) -> String {
        format!("{}cache_window:{series_id}", self.prefix)
    }

    pub fn calendar_series(&self, calendar_id: &Uuid) -> String {
        format!("{}calendar_series:{calendar_id}", self.prefix)
    }
}
