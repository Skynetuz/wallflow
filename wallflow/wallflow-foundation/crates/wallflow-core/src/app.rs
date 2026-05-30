use thiserror::Error;
use wallflow_common::MonitorInfo;
use wallflow_config::AppConfig;
use wallflow_monitor::{platform_monitor_provider, MonitorError, MonitorProvider};

use crate::supervisor::RendererSupervisor;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("monitor error: {0}")]
    Monitor(#[from] MonitorError),

    #[error("supervisor error: {0}")]
    Supervisor(#[from] crate::supervisor::SupervisorError),

    #[error("renderer process error: {0}")]
    RendererProcess(#[from] crate::renderer_process::RendererProcessError),
}

/// The central WallFlow application orchestrator.
///
/// `CoreApp` owns the configuration, the monitor provider, and the renderer
/// supervisor. It is the main entry point for all application logic and
/// coordinates the lifecycle of renderer processes.
pub struct CoreApp {
    config: AppConfig,
    monitor_provider: Box<dyn MonitorProvider>,
    supervisor: RendererSupervisor,
}

impl CoreApp {
    /// Create a new `CoreApp` with the platform-default monitor provider.
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            monitor_provider: platform_monitor_provider(),
            supervisor: RendererSupervisor::new(),
        }
    }

    /// Create a new `CoreApp` with a custom monitor provider (for testing).
    pub fn with_monitor_provider(
        config: AppConfig,
        monitor_provider: Box<dyn MonitorProvider>,
    ) -> Self {
        Self {
            config,
            monitor_provider,
            supervisor: RendererSupervisor::new(),
        }
    }

    /// Create a new `CoreApp` with a custom supervisor (for testing).
    pub fn with_supervisor(
        config: AppConfig,
        monitor_provider: Box<dyn MonitorProvider>,
        supervisor: RendererSupervisor,
    ) -> Self {
        Self {
            config,
            monitor_provider,
            supervisor,
        }
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn monitors(&self) -> Result<Vec<MonitorInfo>, CoreError> {
        Ok(self.monitor_provider.snapshot()?)
    }

    pub fn supervisor(&self) -> &RendererSupervisor {
        &self.supervisor
    }

    pub fn supervisor_mut(&mut self) -> &mut RendererSupervisor {
        &mut self.supervisor
    }
}
