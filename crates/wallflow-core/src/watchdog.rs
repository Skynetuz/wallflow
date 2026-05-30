use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use wallflow_common::{RendererHealth, RendererId, RendererRestartPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogPolicy {
    pub heartbeat_timeout_secs: u64,
    pub max_restarts_per_window: u32,
    pub restart_window_secs: u64,
}

impl WatchdogPolicy {
    pub fn heartbeat_timeout(&self) -> Duration {
        Duration::from_secs(self.heartbeat_timeout_secs)
    }

    pub fn restart_window(&self) -> Duration {
        Duration::from_secs(self.restart_window_secs)
    }
}

impl Default for WatchdogPolicy {
    fn default() -> Self {
        Self {
            heartbeat_timeout_secs: 5,
            max_restarts_per_window: 3,
            restart_window_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RendererHeartbeat {
    pub renderer_id: RendererId,
    pub received_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogDecision {
    KeepRunning,
    RestartRenderer,
    EnterSafeMode,
}

/// Determine the watchdog decision based on the last heartbeat and restart history.
///
/// This function is pure and cloud-testable — it does not perform any I/O.
pub fn decide_from_last_heartbeat(
    policy: WatchdogPolicy,
    last_heartbeat: Option<RendererHeartbeat>,
    now: Instant,
    restarts_in_window: u32,
) -> WatchdogDecision {
    if restarts_in_window >= policy.max_restarts_per_window {
        return WatchdogDecision::EnterSafeMode;
    }

    match last_heartbeat {
        Some(hb) if now.duration_since(hb.received_at) <= policy.heartbeat_timeout() => {
            WatchdogDecision::KeepRunning
        }
        Some(_) | None => WatchdogDecision::RestartRenderer,
    }
}

/// Determine renderer health from the last heartbeat.
///
/// Returns the health classification based on how recent the last heartbeat is
/// relative to the configured timeout, and whether the restart count has been
/// exceeded.
pub fn health_from_heartbeat(
    policy: WatchdogPolicy,
    last_heartbeat: Option<RendererHeartbeat>,
    now: Instant,
    restarts_in_window: u32,
) -> RendererHealth {
    if restarts_in_window >= policy.max_restarts_per_window {
        return RendererHealth::Unhealthy;
    }

    match last_heartbeat {
        Some(hb) if now.duration_since(hb.received_at) <= policy.heartbeat_timeout() => {
            RendererHealth::Healthy
        }
        Some(_) => RendererHealth::Stale,
        None => RendererHealth::Stale,
    }
}

/// Check whether a restart should be attempted based on the restart policy
/// and the current restart count.
pub fn should_attempt_restart(restart_policy: RendererRestartPolicy, restart_count: u32) -> bool {
    match restart_policy {
        RendererRestartPolicy::Never => false,
        RendererRestartPolicy::Always => true,
        RendererRestartPolicy::Limited { max_attempts } => restart_count <= max_attempts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wallflow_common::RendererId;

    #[test]
    fn keeps_running_when_heartbeat_is_fresh() {
        let now = Instant::now();
        let hb = RendererHeartbeat {
            renderer_id: RendererId::new(),
            received_at: now,
        };
        assert_eq!(
            decide_from_last_heartbeat(WatchdogPolicy::default(), Some(hb), now, 0),
            WatchdogDecision::KeepRunning
        );
    }

    #[test]
    fn restarts_when_heartbeat_is_stale() {
        let now = Instant::now();
        let hb = RendererHeartbeat {
            renderer_id: RendererId::new(),
            received_at: now - Duration::from_secs(10),
        };
        assert_eq!(
            decide_from_last_heartbeat(WatchdogPolicy::default(), Some(hb), now, 0),
            WatchdogDecision::RestartRenderer
        );
    }

    #[test]
    fn enters_safe_mode_after_too_many_restarts() {
        let now = Instant::now();
        assert_eq!(
            decide_from_last_heartbeat(WatchdogPolicy::default(), None, now, 3),
            WatchdogDecision::EnterSafeMode
        );
    }

    #[test]
    fn health_healthy_when_fresh() {
        let now = Instant::now();
        let hb = RendererHeartbeat {
            renderer_id: RendererId::new(),
            received_at: now,
        };
        assert_eq!(
            health_from_heartbeat(WatchdogPolicy::default(), Some(hb), now, 0),
            RendererHealth::Healthy
        );
    }

    #[test]
    fn health_stale_when_no_heartbeat() {
        let now = Instant::now();
        assert_eq!(
            health_from_heartbeat(WatchdogPolicy::default(), None, now, 0),
            RendererHealth::Stale
        );
    }

    #[test]
    fn health_unhealthy_when_exceeded_restarts() {
        let now = Instant::now();
        assert_eq!(
            health_from_heartbeat(WatchdogPolicy::default(), None, now, 5),
            RendererHealth::Unhealthy
        );
    }

    #[test]
    fn should_attempt_restart_never() {
        assert!(!should_attempt_restart(RendererRestartPolicy::Never, 0));
        assert!(!should_attempt_restart(RendererRestartPolicy::Never, 100));
    }

    #[test]
    fn should_attempt_restart_always() {
        assert!(should_attempt_restart(RendererRestartPolicy::Always, 0));
        assert!(should_attempt_restart(RendererRestartPolicy::Always, 100));
    }

    #[test]
    fn should_attempt_restart_limited() {
        let policy = RendererRestartPolicy::Limited { max_attempts: 3 };
        assert!(should_attempt_restart(policy, 0));
        assert!(should_attempt_restart(policy, 3));
        assert!(!should_attempt_restart(policy, 4));
    }

    #[test]
    fn no_heartbeat_means_restart() {
        let now = Instant::now();
        assert_eq!(
            decide_from_last_heartbeat(WatchdogPolicy::default(), None, now, 0),
            WatchdogDecision::RestartRenderer
        );
    }
}
