use serde::{Deserialize, Serialize};

pub(crate) const SERVICE_RUNTIME_EVENT: &str = "service-runtime-updated";
pub(crate) const SERVICE_LOG_EVENT: &str = "service-log-appended";
pub(crate) const SERVICE_LOGS_CLEARED_EVENT: &str = "service-logs-cleared";
pub(crate) const MAX_LOG_ENTRIES: usize = 2_000;
pub(crate) const MAX_LOG_BUFFER_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_LOG_ENTRY_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ServiceRuntimeStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Exited,
    Failed,
}

impl ServiceRuntimeStatus {
    pub(crate) fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Exited => "exited",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceRuntimeSnapshot {
    pub project_id: String,
    pub service_id: String,
    pub run_id: Option<String>,
    pub runtime_revision: u64,
    pub status: ServiceRuntimeStatus,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub exit_code: Option<u32>,
    pub error: Option<String>,
}

impl ServiceRuntimeSnapshot {
    pub(crate) fn stopped(project_id: &str, service_id: &str) -> Self {
        Self {
            project_id: project_id.to_owned(),
            service_id: service_id.to_owned(),
            run_id: None,
            runtime_revision: 0,
            status: ServiceRuntimeStatus::Stopped,
            pid: None,
            started_at: None,
            exit_code: None,
            error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LogSource {
    Stdout,
    Stderr,
}

impl LogSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceLogEntry {
    pub sequence: u64,
    pub timestamp: String,
    pub source: LogSource,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceLogEvent {
    pub project_id: String,
    pub service_id: String,
    pub run_id: String,
    pub logs_revision: u64,
    pub entry: ServiceLogEntry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceLogsSnapshot {
    pub project_id: String,
    pub service_id: String,
    pub run_id: Option<String>,
    pub logs_revision: u64,
    pub entries: Vec<ServiceLogEntry>,
}
