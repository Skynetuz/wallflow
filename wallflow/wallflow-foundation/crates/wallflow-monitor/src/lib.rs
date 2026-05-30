//! Monitor enumeration and topology diffing.

mod diff;
mod provider;

pub use diff::diff_monitor_snapshots;
pub use provider::{platform_monitor_provider, MonitorError, MonitorEvent, MonitorProvider};
