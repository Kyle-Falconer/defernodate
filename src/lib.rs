//! # defernodate
//!
//! A pure, synchronous library for RRULE expansion and calendar series
//! operations. No I/O, no runtime dependencies — the caller supplies
//! ids, timestamps, and storage.
//!
//! See README.md for migration notes from 0.1.x (which embedded Redis).

pub mod config;
pub mod error;
pub mod expand;
pub mod model;
pub mod pure;

pub use config::CacheConfig;
pub use error::{Error, Result};
pub use expand::expand_series;
pub use model::{CacheWindow, CreateSeries, Instance, Override, Series, UpdateSeries};
pub use pure::{apply_update, build_series, get_instance, split_series};
