use std::collections::HashMap;
use wallflow_common::{MonitorId, MonitorInfo};

use crate::MonitorEvent;

/// Builds monitor topology events from old and new snapshots.
pub fn diff_monitor_snapshots(old: &[MonitorInfo], new: &[MonitorInfo]) -> Vec<MonitorEvent> {
    let old_by_id: HashMap<&MonitorId, &MonitorInfo> = old.iter().map(|m| (&m.id, m)).collect();
    let new_by_id: HashMap<&MonitorId, &MonitorInfo> = new.iter().map(|m| (&m.id, m)).collect();

    let mut events = Vec::new();

    for monitor in new {
        match old_by_id.get(&monitor.id) {
            None => events.push(MonitorEvent::Added(monitor.clone())),
            Some(previous) if *previous != monitor => {
                events.push(MonitorEvent::Changed(monitor.clone()))
            }
            Some(_) => {}
        }
    }

    for monitor in old {
        if !new_by_id.contains_key(&monitor.id) {
            events.push(MonitorEvent::Removed {
                id: monitor.id.clone(),
            });
        }
    }

    if !events.is_empty() {
        events.push(MonitorEvent::TopologyChanged(new.to_vec()));
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use wallflow_common::{MonitorId, MonitorInfo, MonitorPosition, MonitorSize};

    fn monitor(id: &str, primary: bool, w: u32) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(id.to_owned()),
            name: id.to_owned(),
            is_primary: primary,
            position: MonitorPosition { x: 0, y: 0 },
            size_px: MonitorSize {
                width: w,
                height: 1080,
            },
            work_area_px: MonitorSize {
                width: w,
                height: 1040,
            },
            scale_factor: 1.0,
            refresh_rate_millihz: Some(60000),
        }
    }

    #[test]
    fn detects_added_monitor() {
        let old = vec![monitor("A", true, 1920)];
        let new = vec![monitor("A", true, 1920), monitor("B", false, 1920)];
        let events = diff_monitor_snapshots(&old, &new);
        assert!(matches!(events.first(), Some(MonitorEvent::Added(m)) if m.id.0 == "B"));
        assert!(matches!(
            events.last(),
            Some(MonitorEvent::TopologyChanged(_))
        ));
    }

    #[test]
    fn detects_changed_monitor() {
        let old = vec![monitor("A", true, 1920)];
        let new = vec![monitor("A", true, 2560)];
        let events = diff_monitor_snapshots(&old, &new);
        assert!(
            matches!(events.first(), Some(MonitorEvent::Changed(m)) if m.size_px.width == 2560)
        );
    }

    #[test]
    fn emits_no_events_for_equal_snapshots() {
        let old = vec![monitor("A", true, 1920)];
        let new = old.clone();
        let events = diff_monitor_snapshots(&old, &new);
        assert!(events.is_empty());
    }
}
