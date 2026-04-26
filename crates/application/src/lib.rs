use chrono::{DateTime, Utc};
use pingorahub_domain::{NodeStatus, ReleaseManifest};

#[derive(Debug, Clone)]
pub struct HeartbeatPolicy {
    pub suspect_after_missed: u32,
    pub offline_after_missed: u32,
}

impl Default for HeartbeatPolicy {
    fn default() -> Self {
        Self {
            suspect_after_missed: 2,
            offline_after_missed: 4,
        }
    }
}

pub struct NodeLifecycleService;

impl NodeLifecycleService {
    pub fn evaluate_status(missed_heartbeats: u32, policy: &HeartbeatPolicy) -> NodeStatus {
        if missed_heartbeats >= policy.offline_after_missed {
            NodeStatus::Offline
        } else if missed_heartbeats >= policy.suspect_after_missed {
            NodeStatus::Suspect
        } else {
            NodeStatus::Online
        }
    }
}

pub struct ReleasePlanner;

impl ReleasePlanner {
    pub fn build_release_version(scope: &str, issued_at: DateTime<Utc>, serial: u64) -> String {
        format!(
            "rel-{}-{}-{serial:03}",
            scope,
            issued_at.format("%Y%m%d%H%M%S")
        )
    }

    pub fn summarize(manifest: &ReleaseManifest) -> String {
        format!(
            "release={} scope={} targets={} sites={}",
            manifest.release_version,
            manifest.scope,
            manifest.targets.len(),
            manifest.sites.len()
        )
    }
}
