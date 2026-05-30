use thiserror::Error;
use wallflow_common::MonitorInfo;
use wallflow_config::AppConfig;
use wallflow_monitor::{platform_monitor_provider, MonitorError, MonitorProvider};

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("monitor error: {0}")]
    Monitor(#[from] MonitorError),
}

pub struct CoreApp {
    config: AppConfig,
    monitor_provider: Box<dyn MonitorProvider>,
}

impl CoreApp {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            monitor_provider: platform_monitor_provider(),
        }
    }

    pub fn with_monitor_provider(
        config: AppConfig,
        monitor_provider: Box<dyn MonitorProvider>,
    ) -> Self {
        Self {
            config,
            monitor_provider,
        }
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn monitors(&self) -> Result<Vec<MonitorInfo>, CoreError> {
        Ok(self.monitor_provider.snapshot()?)
    }
}
