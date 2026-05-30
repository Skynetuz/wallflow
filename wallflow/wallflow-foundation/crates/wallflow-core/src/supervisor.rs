//! Renderer supervisor: manages the lifecycle of one renderer process.
//!
//! The `RendererSupervisor` tracks multiple renderer processes, records
//! heartbeats, detects stale renderers, applies restart policies, and
//! generates structured reports. All logic is cloud-testable.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};
use wallflow_common::{
    MonitorId, RendererAssignment, RendererHealth, RendererId, RendererRestartPolicy, RendererState,
};

use crate::renderer_process::{RendererProcessError, RendererProcessManager};
use crate::watchdog::{
    decide_from_last_heartbeat, RendererHeartbeat, WatchdogDecision, WatchdogPolicy,
};

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("renderer {0} not found")]
    RendererNotFound(RendererId),

    #[error("renderer process error: {0}")]
    Process(#[from] RendererProcessError),

    #[error("renderer {0} is already in state {1:?}")]
    InvalidState(RendererId, RendererState),

    #[error("renderer {0} already assigned to monitor {1:?}")]
    AlreadyAssigned(RendererId, MonitorId),

    #[error("wallpaper apply failed for renderer {0}: {1}")]
    ApplyFailed(RendererId, String),

    #[error("renderer {0} is not running, cannot apply wallpaper")]
    NotRunning(RendererId),
}

/// Current status of a single renderer within the supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererStatus {
    /// Renderer process is starting up.
    Starting,
    /// Renderer is running and healthy.
    Running,
    /// Renderer is running but heartbeat is stale.
    Stale,
    /// Renderer has been paused.
    Paused,
    /// Renderer is in the process of stopping.
    Stopping,
    /// Renderer has stopped normally.
    Stopped,
    /// Renderer has crashed and may be eligible for restart.
    Crashed { restart_count: u32 },
    /// Renderer has exceeded the maximum restart count and is in safe mode.
    SafeMode,
}

impl From<RendererStatus> for RendererState {
    fn from(status: RendererStatus) -> RendererState {
        match status {
            RendererStatus::Starting => RendererState::Starting,
            RendererStatus::Running | RendererStatus::Stale => RendererState::Running,
            RendererStatus::Paused => RendererState::Paused,
            RendererStatus::Stopping => RendererState::Stopping,
            RendererStatus::Stopped => RendererState::Stopped,
            RendererStatus::Crashed { .. } => RendererState::Crashed,
            RendererStatus::SafeMode => RendererState::Crashed,
        }
    }
}

impl From<RendererStatus> for RendererHealth {
    fn from(status: RendererStatus) -> RendererHealth {
        match status {
            RendererStatus::Running => RendererHealth::Healthy,
            RendererStatus::Stale => RendererHealth::Stale,
            RendererStatus::SafeMode => RendererHealth::Unhealthy,
            RendererStatus::Crashed { .. } => RendererHealth::Unhealthy,
            _ => RendererHealth::Stale,
        }
    }
}

/// Opaque handle to a renderer tracked by the supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererHandle {
    pub renderer_id: RendererId,
    pub monitor_id: MonitorId,
    pub status: RendererStatus,
}

/// Per-renderer record kept inside the supervisor.
struct RendererEntry {
    status: RendererStatus,
    assignment: RendererAssignment,
    process: RendererProcessManager,
    last_heartbeat: Option<RendererHeartbeat>,
    restart_count: u32,
    restart_window_start: Instant,
    launched_at: Instant,
    /// The wallpaper currently applied to this renderer (if any).
    applied_wallpaper_id: Option<wallflow_common::WallpaperId>,
}

/// Report for a single renderer, used for structured output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererReport {
    pub renderer_id: RendererId,
    pub monitor_id: MonitorId,
    pub status: RendererStatus,
    pub health: RendererHealth,
    pub restart_count: u32,
    pub uptime_ms: Option<u64>,
}

/// Snapshot of all renderers managed by the supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorSnapshot {
    pub renderers: Vec<RendererReport>,
    pub total: usize,
    pub healthy: usize,
    pub stale: usize,
    pub crashed: usize,
}

/// Overall supervisor report with policy information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorReport {
    pub snapshot: SupervisorSnapshot,
    pub watchdog_policy: WatchdogPolicy,
    pub restart_policy: RendererRestartPolicy,
}

/// Manages the lifecycle of renderer processes.
///
/// The supervisor tracks each renderer process, records heartbeats, detects
/// stale or crashed renderers, and applies the restart policy. It is designed
/// to be used from an async context but the internal bookkeeping is synchronous
/// so it can be unit-tested without tokio.
pub struct RendererSupervisor {
    entries: HashMap<RendererId, RendererEntry>,
    watchdog_policy: WatchdogPolicy,
    restart_policy: RendererRestartPolicy,
}

impl RendererSupervisor {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            watchdog_policy: WatchdogPolicy::default(),
            restart_policy: RendererRestartPolicy::default(),
        }
    }

    pub fn with_watchdog_policy(mut self, policy: WatchdogPolicy) -> Self {
        self.watchdog_policy = policy;
        self
    }

    pub fn with_restart_policy(mut self, policy: RendererRestartPolicy) -> Self {
        self.restart_policy = policy;
        self
    }

    /// Register a new renderer assignment. The renderer starts in `Starting` state.
    ///
    /// The caller is responsible for actually spawning the process via
    /// `RendererProcessManager`.
    pub fn register_renderer(&mut self, assignment: RendererAssignment) -> RendererHandle {
        let renderer_id = assignment.renderer_id;
        let monitor_id = assignment.monitor_id.clone();
        let entry = RendererEntry {
            status: RendererStatus::Starting,
            assignment,
            process: RendererProcessManager::new(),
            last_heartbeat: None,
            restart_count: 0,
            restart_window_start: Instant::now(),
            launched_at: Instant::now(),
            applied_wallpaper_id: None,
        };
        self.entries.insert(renderer_id, entry);
        debug!(?renderer_id, ?monitor_id, "renderer registered");
        RendererHandle {
            renderer_id,
            monitor_id,
            status: RendererStatus::Starting,
        }
    }

    /// Mark a renderer as running (after it sends its first heartbeat or Ready event).
    pub fn mark_running(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        entry.status = RendererStatus::Running;
        entry.launched_at = Instant::now();
        debug!(?renderer_id, "renderer marked running");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Running,
        })
    }

    /// Record a heartbeat from a renderer process.
    pub fn mark_heartbeat(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        let hb = RendererHeartbeat {
            renderer_id,
            received_at: Instant::now(),
        };
        entry.last_heartbeat = Some(hb);
        if entry.status == RendererStatus::Stale || entry.status == RendererStatus::Starting {
            entry.status = RendererStatus::Running;
            debug!(?renderer_id, "renderer recovered via heartbeat");
        }
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: entry.status.clone(),
        })
    }

    /// Mark a renderer as paused.
    pub fn mark_paused(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        if entry.status != RendererStatus::Running && entry.status != RendererStatus::Stale {
            return Err(SupervisorError::InvalidState(
                renderer_id,
                entry.status.clone().into(),
            ));
        }
        entry.status = RendererStatus::Paused;
        debug!(?renderer_id, "renderer paused");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Paused,
        })
    }

    /// Mark a renderer as resumed from pause.
    pub fn mark_resumed(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        if entry.status != RendererStatus::Paused {
            return Err(SupervisorError::InvalidState(
                renderer_id,
                entry.status.clone().into(),
            ));
        }
        entry.status = RendererStatus::Running;
        debug!(?renderer_id, "renderer resumed");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Running,
        })
    }

    /// Mark a renderer as stopping (graceful shutdown in progress).
    pub fn mark_stopping(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        entry.status = RendererStatus::Stopping;
        debug!(?renderer_id, "renderer stopping");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Stopping,
        })
    }

    /// Mark a renderer as stopped (normal shutdown).
    pub fn mark_stopped(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        entry.status = RendererStatus::Stopped;
        entry.last_heartbeat = None;
        info!(?renderer_id, "renderer stopped");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Stopped,
        })
    }

    /// Mark a renderer as crashed.
    pub fn mark_crashed(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        let new_count = entry.restart_count + 1;
        entry.restart_count = new_count;
        entry.status = RendererStatus::Crashed {
            restart_count: new_count,
        };
        entry.last_heartbeat = None;
        warn!(?renderer_id, restart_count = new_count, "renderer crashed");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Crashed {
                restart_count: new_count,
            },
        })
    }

    /// Detect stale renderers by checking heartbeat recency.
    ///
    /// Returns a list of renderer IDs that have become stale.
    pub fn detect_stale(&mut self) -> Vec<RendererId> {
        let now = Instant::now();
        let mut stale_ids = Vec::new();
        for (id, entry) in &mut self.entries {
            if entry.status == RendererStatus::Running {
                let decision = decide_from_last_heartbeat(
                    self.watchdog_policy,
                    entry.last_heartbeat,
                    now,
                    entry.restart_count,
                );
                if decision == WatchdogDecision::RestartRenderer {
                    entry.status = RendererStatus::Stale;
                    stale_ids.push(*id);
                    debug!(renderer_id = ?id, "renderer detected as stale");
                }
            }
        }
        stale_ids
    }

    /// Check whether a crashed renderer should be restarted based on the restart policy.
    pub fn should_restart(&self, renderer_id: RendererId) -> Result<bool, SupervisorError> {
        let entry = self
            .entries
            .get(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        if !matches!(entry.status, RendererStatus::Crashed { .. }) {
            return Ok(false);
        }
        match self.restart_policy {
            RendererRestartPolicy::Never => Ok(false),
            RendererRestartPolicy::Always => Ok(true),
            RendererRestartPolicy::Limited { max_attempts } => {
                Ok(entry.restart_count <= max_attempts)
            }
        }
    }

    /// Recover a crashed renderer by resetting its state to Starting.
    ///
    /// The caller is responsible for respawning the process.
    pub fn recover(&mut self, renderer_id: RendererId) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        if !matches!(entry.status, RendererStatus::Crashed { .. }) {
            return Err(SupervisorError::InvalidState(
                renderer_id,
                entry.status.clone().into(),
            ));
        }
        // Reset the restart window if enough time has elapsed
        let now = Instant::now();
        if now.duration_since(entry.restart_window_start) > self.watchdog_policy.restart_window() {
            entry.restart_count = 0;
            entry.restart_window_start = now;
            debug!(?renderer_id, "restart window reset");
        }
        entry.status = RendererStatus::Starting;
        entry.last_heartbeat = None;
        info!(?renderer_id, "renderer recovery initiated");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: RendererStatus::Starting,
        })
    }

    /// Remove a renderer from the supervisor. Returns the old handle if it existed.
    pub fn deregister(&mut self, renderer_id: RendererId) -> Option<RendererHandle> {
        self.entries
            .remove(&renderer_id)
            .map(|entry| RendererHandle {
                renderer_id,
                monitor_id: entry.assignment.monitor_id,
                status: entry.status,
            })
    }

    /// Get a snapshot of all renderer states.
    pub fn snapshot(&self) -> SupervisorSnapshot {
        let now = Instant::now();
        let mut renderers = Vec::new();
        let mut healthy = 0;
        let mut stale = 0;
        let mut crashed = 0;

        for (id, entry) in &self.entries {
            let health = RendererHealth::from(entry.status.clone());
            let uptime_ms = if entry.status == RendererStatus::Running
                || entry.status == RendererStatus::Stale
                || entry.status == RendererStatus::Paused
            {
                Some(now.duration_since(entry.launched_at).as_millis() as u64)
            } else {
                None
            };

            match &entry.status {
                RendererStatus::Running => healthy += 1,
                RendererStatus::Stale => stale += 1,
                RendererStatus::Crashed { .. } | RendererStatus::SafeMode => crashed += 1,
                _ => {}
            }

            renderers.push(RendererReport {
                renderer_id: *id,
                monitor_id: entry.assignment.monitor_id.clone(),
                status: entry.status.clone(),
                health,
                restart_count: entry.restart_count,
                uptime_ms,
            });
        }

        SupervisorSnapshot {
            total: renderers.len(),
            renderers,
            healthy,
            stale,
            crashed,
        }
    }

    /// Generate a full supervisor report including policy information.
    pub fn report(&self) -> SupervisorReport {
        SupervisorReport {
            snapshot: self.snapshot(),
            watchdog_policy: self.watchdog_policy,
            restart_policy: self.restart_policy,
        }
    }

    /// Get the process manager for a specific renderer (for spawning/killing).
    pub fn process_manager(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<&mut RendererProcessManager, SupervisorError> {
        self.entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))
            .map(|e| &mut e.process)
    }

    /// Get the current restart count for a renderer.
    pub fn restart_count(&self, renderer_id: RendererId) -> Result<u32, SupervisorError> {
        self.entries
            .get(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))
            .map(|e| e.restart_count)
    }

    /// Request a wallpaper apply for a renderer.
    ///
    /// Validates that the renderer is in a running or paused state before
    /// accepting the apply request. Returns the renderer handle if the
    /// request is valid. The caller is responsible for sending the actual
    /// IPC command to the renderer process.
    pub fn apply_wallpaper(
        &mut self,
        renderer_id: RendererId,
        wallpaper_id: wallflow_common::WallpaperId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;

        if entry.status != RendererStatus::Running
            && entry.status != RendererStatus::Paused
            && entry.status != RendererStatus::Stale
        {
            return Err(SupervisorError::NotRunning(renderer_id));
        }

        entry.applied_wallpaper_id = Some(wallpaper_id);
        info!(?renderer_id, ?wallpaper_id, "wallpaper apply requested");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: entry.status.clone(),
        })
    }

    /// Record that a wallpaper was successfully applied to a renderer.
    pub fn mark_wallpaper_applied(
        &mut self,
        renderer_id: RendererId,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        debug!(
            ?renderer_id,
            wallpaper_id = ?entry.applied_wallpaper_id,
            "wallpaper apply confirmed"
        );
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: entry.status.clone(),
        })
    }

    /// Record that a wallpaper apply failed for a renderer.
    pub fn mark_wallpaper_apply_failed(
        &mut self,
        renderer_id: RendererId,
        reason: String,
    ) -> Result<RendererHandle, SupervisorError> {
        let entry = self
            .entries
            .get_mut(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))?;
        entry.applied_wallpaper_id = None;
        warn!(?renderer_id, %reason, "wallpaper apply failed");
        Ok(RendererHandle {
            renderer_id,
            monitor_id: entry.assignment.monitor_id.clone(),
            status: entry.status.clone(),
        })
    }

    /// Get the currently applied wallpaper ID for a renderer.
    pub fn applied_wallpaper(
        &self,
        renderer_id: RendererId,
    ) -> Result<Option<wallflow_common::WallpaperId>, SupervisorError> {
        self.entries
            .get(&renderer_id)
            .ok_or(SupervisorError::RendererNotFound(renderer_id))
            .map(|e| e.applied_wallpaper_id)
    }
}

impl Default for RendererSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_assignment() -> RendererAssignment {
        RendererAssignment::new(RendererId::new(), MonitorId("test-mon".into()))
    }

    #[test]
    fn register_and_mark_running() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        let handle = supervisor.register_renderer(assignment);
        assert_eq!(handle.status, RendererStatus::Starting);

        let handle = supervisor.mark_running(rid).expect("mark running");
        assert_eq!(handle.status, RendererStatus::Running);
    }

    #[test]
    fn mark_heartbeat_transitions_from_starting() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        let handle = supervisor.mark_heartbeat(rid).expect("heartbeat");
        assert_eq!(handle.status, RendererStatus::Running);
    }

    #[test]
    fn pause_and_resume() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");

        let handle = supervisor.mark_paused(rid).expect("pause");
        assert_eq!(handle.status, RendererStatus::Paused);

        let handle = supervisor.mark_resumed(rid).expect("resume");
        assert_eq!(handle.status, RendererStatus::Running);
    }

    #[test]
    fn pause_rejects_non_running() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        // Still in Starting state — cannot pause
        let result = supervisor.mark_paused(rid);
        assert!(result.is_err());
    }

    #[test]
    fn crash_and_restart_policy_limited() {
        let mut supervisor = RendererSupervisor::new()
            .with_restart_policy(RendererRestartPolicy::Limited { max_attempts: 2 });
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");

        // First crash
        supervisor.mark_crashed(rid).expect("crash");
        assert!(supervisor.should_restart(rid).expect("should restart"));
        supervisor.recover(rid).expect("recover");

        // Restart and crash again (restart_count = 2)
        supervisor.mark_running(rid).expect("running 2");
        supervisor.mark_crashed(rid).expect("crash 2");
        // restart_count is now 2, max_attempts is 2, so should_restart = true (2 <= 2)
        assert!(supervisor.should_restart(rid).expect("should restart 2"));
        supervisor.recover(rid).expect("recover 2");

        // Third crash (restart_count = 3, exceeds max_attempts)
        supervisor.mark_running(rid).expect("running 3");
        supervisor.mark_crashed(rid).expect("crash 3");
        assert!(!supervisor.should_restart(rid).expect("should not restart"));
    }

    #[test]
    fn crash_with_never_restart_policy() {
        let mut supervisor =
            RendererSupervisor::new().with_restart_policy(RendererRestartPolicy::Never);
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");
        supervisor.mark_crashed(rid).expect("crash");
        assert!(!supervisor.should_restart(rid).expect("no restart"));
    }

    #[test]
    fn crash_with_always_restart_policy() {
        let mut supervisor =
            RendererSupervisor::new().with_restart_policy(RendererRestartPolicy::Always);
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");

        // Crash many times
        for _ in 0..10 {
            supervisor.mark_crashed(rid).expect("crash");
            assert!(supervisor.should_restart(rid).expect("always restart"));
            supervisor.recover(rid).expect("recover");
            supervisor.mark_running(rid).expect("running");
        }
    }

    #[test]
    fn detect_stale_renderer() {
        let mut supervisor = RendererSupervisor::new().with_watchdog_policy(WatchdogPolicy {
            heartbeat_timeout_secs: 1,
            max_restarts_per_window: 3,
            restart_window_secs: 60,
        });
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");

        // Heartbeat was just received — not stale
        supervisor.mark_heartbeat(rid).expect("heartbeat");
        let stale = supervisor.detect_stale();
        assert!(stale.is_empty());

        // Wait for heartbeat to become stale (timeout is 1 second)
        std::thread::sleep(Duration::from_millis(1500));
        let stale = supervisor.detect_stale();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], rid);
    }

    #[test]
    fn snapshot_report() {
        let mut supervisor = RendererSupervisor::new();
        let assignment1 = test_assignment();
        let rid1 = assignment1.renderer_id;
        let assignment2 = RendererAssignment::new(RendererId::new(), MonitorId("mon-2".into()));
        let rid2 = assignment2.renderer_id;

        supervisor.register_renderer(assignment1);
        supervisor.register_renderer(assignment2);
        supervisor.mark_running(rid1).expect("running 1");
        supervisor.mark_running(rid2).expect("running 2");

        let snapshot = supervisor.snapshot();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.healthy, 2);
        assert_eq!(snapshot.stale, 0);
        assert_eq!(snapshot.crashed, 0);
    }

    #[test]
    fn deregister_removes_renderer() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        let handle = supervisor.deregister(rid);
        assert!(handle.is_some());

        // Now it should be gone
        let result = supervisor.mark_running(rid);
        assert!(result.is_err());
    }

    #[test]
    fn stop_and_stopped() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");
        supervisor.mark_stopping(rid).expect("stopping");
        supervisor.mark_stopped(rid).expect("stopped");

        let snapshot = supervisor.snapshot();
        assert_eq!(snapshot.healthy, 0);
        assert_eq!(snapshot.total, 1);
    }

    #[test]
    fn renderer_not_found_errors() {
        let mut supervisor = RendererSupervisor::new();
        let bogus = RendererId::new();
        assert!(supervisor.mark_running(bogus).is_err());
        assert!(supervisor.mark_heartbeat(bogus).is_err());
        assert!(supervisor.mark_paused(bogus).is_err());
        assert!(supervisor.mark_crashed(bogus).is_err());
        assert!(supervisor.should_restart(bogus).is_err());
        assert!(supervisor.recover(bogus).is_err());
    }

    #[test]
    fn recover_rejects_non_crashed() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");

        // Cannot recover a running renderer
        let result = supervisor.recover(rid);
        assert!(result.is_err());
    }

    #[test]
    fn supervisor_report_includes_policies() {
        let policy = WatchdogPolicy {
            heartbeat_timeout_secs: 10,
            max_restarts_per_window: 5,
            restart_window_secs: 120,
        };
        let supervisor = RendererSupervisor::new()
            .with_watchdog_policy(policy)
            .with_restart_policy(RendererRestartPolicy::Limited { max_attempts: 5 });
        let report = supervisor.report();
        assert_eq!(report.watchdog_policy.heartbeat_timeout_secs, 10);
        assert_eq!(
            report.restart_policy,
            RendererRestartPolicy::Limited { max_attempts: 5 }
        );
    }

    #[test]
    fn health_from_status() {
        assert_eq!(
            RendererHealth::from(RendererStatus::Running),
            RendererHealth::Healthy
        );
        assert_eq!(
            RendererHealth::from(RendererStatus::Stale),
            RendererHealth::Stale
        );
        assert_eq!(
            RendererHealth::from(RendererStatus::Crashed { restart_count: 1 }),
            RendererHealth::Unhealthy
        );
        assert_eq!(
            RendererHealth::from(RendererStatus::SafeMode),
            RendererHealth::Unhealthy
        );
        assert_eq!(
            RendererHealth::from(RendererStatus::Starting),
            RendererHealth::Stale
        );
        assert_eq!(
            RendererHealth::from(RendererStatus::Paused),
            RendererHealth::Stale
        );
    }

    #[test]
    fn apply_wallpaper_state_transition() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;
        let wid = wallflow_common::WallpaperId::new();

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");

        let handle = supervisor.apply_wallpaper(rid, wid).expect("apply");
        assert_eq!(handle.status, RendererStatus::Running);

        // The wallpaper_id should be recorded
        let applied = supervisor.applied_wallpaper(rid).expect("get applied");
        assert_eq!(applied, Some(wid));
    }

    #[test]
    fn apply_wallpaper_rejects_non_running() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;
        let wid = wallflow_common::WallpaperId::new();

        supervisor.register_renderer(assignment);
        // Still in Starting state — cannot apply
        let result = supervisor.apply_wallpaper(rid, wid);
        assert!(result.is_err());
    }

    #[test]
    fn mark_wallpaper_applied_success() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;
        let wid = wallflow_common::WallpaperId::new();

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");
        supervisor.apply_wallpaper(rid, wid).expect("apply");

        let handle = supervisor.mark_wallpaper_applied(rid).expect("confirmed");
        assert_eq!(handle.status, RendererStatus::Running);
    }

    #[test]
    fn mark_wallpaper_apply_failed_clears_applied() {
        let mut supervisor = RendererSupervisor::new();
        let assignment = test_assignment();
        let rid = assignment.renderer_id;
        let wid = wallflow_common::WallpaperId::new();

        supervisor.register_renderer(assignment);
        supervisor.mark_running(rid).expect("running");
        supervisor.apply_wallpaper(rid, wid).expect("apply");

        let handle = supervisor
            .mark_wallpaper_apply_failed(rid, "image not found".into())
            .expect("failed mark");
        assert_eq!(handle.status, RendererStatus::Running);

        // Applied wallpaper should be cleared
        let applied = supervisor.applied_wallpaper(rid).expect("get applied");
        assert_eq!(applied, None);
    }
}
