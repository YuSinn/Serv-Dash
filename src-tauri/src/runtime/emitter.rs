use crate::runtime::model::{
    ServiceLogEvent, ServiceLogsSnapshot, ServiceRuntimeSnapshot, SERVICE_LOGS_CLEARED_EVENT,
    SERVICE_LOG_EVENT, SERVICE_RUNTIME_EVENT,
};
use tauri::{AppHandle, Emitter};

pub(crate) trait RuntimeEventEmitter: Send + Sync {
    fn emit_runtime(&self, snapshot: &ServiceRuntimeSnapshot) -> Result<(), String>;
    fn emit_log(&self, event: &ServiceLogEvent) -> Result<(), String>;
    fn emit_logs_cleared(&self, snapshot: &ServiceLogsSnapshot) -> Result<(), String>;
}

pub(super) struct TauriRuntimeEventEmitter {
    app: AppHandle,
}

impl TauriRuntimeEventEmitter {
    pub(super) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeEventEmitter for TauriRuntimeEventEmitter {
    fn emit_runtime(&self, snapshot: &ServiceRuntimeSnapshot) -> Result<(), String> {
        self.app
            .emit(SERVICE_RUNTIME_EVENT, snapshot)
            .map_err(|error| error.to_string())
    }

    fn emit_log(&self, event: &ServiceLogEvent) -> Result<(), String> {
        self.app
            .emit(SERVICE_LOG_EVENT, event)
            .map_err(|error| error.to_string())
    }

    fn emit_logs_cleared(&self, snapshot: &ServiceLogsSnapshot) -> Result<(), String> {
        self.app
            .emit(SERVICE_LOGS_CLEARED_EVENT, snapshot)
            .map_err(|error| error.to_string())
    }
}
