use std::time::{Duration, Instant};
use wallflow_common::RendererId;

#[derive(Debug, Clone, Copy)]
pub struct WatchdogPolicy {
    pub heartbeat_timeout: Duration,
    pub max_restarts_per_window: u32,
    pub restart_window: Duration,
}

impl Default for WatchdogPolicy {
    fn default() -> Self {
        Self {
            heartbeat_timeout: Duration::from_secs(5),
            max_restarts_per_window: 3,
            restart_window: Duration::from_secs(60),
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
        Some(hb) if now.duration_since(hb.received_at) <= policy.heartbeat_timeout => {
            WatchdogDecision::KeepRunning
        }
        Some(_) | None => WatchdogDecision::RestartRenderer,
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
}
