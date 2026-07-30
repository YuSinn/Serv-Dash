use crate::projects::ProjectError;
use crate::runtime::manager::{
    lock_record, logs_snapshot, next_logs_revision, next_runtime_revision, runtime_lock_error,
    snapshot, RuntimeManager, ServiceKey, CLOSE_WAIT,
};
use crate::runtime::model::{ServiceLogsSnapshot, ServiceRuntimeSnapshot, ServiceRuntimeStatus};
use crate::runtime::windows_process::{RunControl, SpawnedProcess};
use chrono::{SecondsFormat, Utc};
use std::sync::Arc;

pub(super) enum ResumeDisposition {
    Running(ServiceRuntimeSnapshot),
    StopRequested,
}

impl RuntimeManager {
    pub(crate) fn get_runtime(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        self.get_runtime_for_key(&ServiceKey::new(project_id, service_id))
    }

    pub(crate) fn get_logs(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<ServiceLogsSnapshot, ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let Some(entry) = self.existing_entry(&key)? else {
            return Ok(ServiceLogsSnapshot {
                project_id: project_id.to_owned(),
                service_id: service_id.to_owned(),
                run_id: None,
                logs_revision: 0,
                entries: Vec::new(),
            });
        };
        let record = lock_record(&entry)?;
        Ok(logs_snapshot(&key, &record))
    }

    pub(crate) fn clear_logs(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<ServiceLogsSnapshot, ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let result = {
            let mut record = lock_record(&entry)?;
            let logs_revision = next_logs_revision(&record)?;
            record.logs.clear();
            record.log_bytes = 0;
            record.logs_revision = logs_revision;
            logs_snapshot(&key, &record)
        };
        self.emit_logs_cleared_snapshot(result.clone());
        Ok(result)
    }

    pub(crate) fn ensure_service_inactive(
        &self,
        project_id: &str,
        service_id: &str,
        action: &'static str,
    ) -> Result<(), ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let Some(entry) = self.existing_entry(&key)? else {
            return Ok(());
        };
        let record = lock_record(&entry)?;
        if record.status.is_active() {
            Err(ProjectError::ServiceRuntimeActive {
                project_id: project_id.to_owned(),
                service_id: service_id.to_owned(),
                action,
                status: record.status.as_str().to_owned(),
            })
        } else {
            Ok(())
        }
    }

    pub(crate) fn ensure_project_inactive(
        &self,
        project_id: &str,
        action: &'static str,
    ) -> Result<(), ProjectError> {
        let entries = {
            let entries = self
                .inner
                .entries
                .lock()
                .map_err(|_| runtime_lock_error())?;
            entries
                .iter()
                .filter(|(key, _)| key.project_id == project_id)
                .map(|(key, entry)| (key.clone(), Arc::clone(entry)))
                .collect::<Vec<_>>()
        };

        for (key, entry) in entries {
            let record = lock_record(&entry)?;
            if record.status.is_active() {
                return Err(ProjectError::ProjectRuntimeActive {
                    project_id: project_id.to_owned(),
                    service_id: key.service_id,
                    action,
                    status: record.status.as_str().to_owned(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn forget_service(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<(), ProjectError> {
        self.inner
            .entries
            .lock()
            .map_err(|_| runtime_lock_error())?
            .remove(&ServiceKey::new(project_id, service_id));
        Ok(())
    }

    pub(crate) fn forget_project(&self, project_id: &str) -> Result<(), ProjectError> {
        self.inner
            .entries
            .lock()
            .map_err(|_| runtime_lock_error())?
            .retain(|key, _| key.project_id != project_id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn has_control(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<bool, ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let Some(entry) = self.existing_entry(&key)? else {
            return Ok(false);
        };
        let has_control = lock_record(&entry)?.control.is_some();
        Ok(has_control)
    }

    pub(super) fn install_control(
        &self,
        key: &ServiceKey,
        run_id: &str,
        pid: u32,
        control: Arc<RunControl>,
    ) -> Result<(bool, ServiceRuntimeSnapshot), ProjectError> {
        let lifecycle = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| runtime_lock_error())?;
        if lifecycle.shutting_down {
            return Err(ProjectError::RuntimeShuttingDown);
        }
        let entry = self.entry(key)?;
        let mut record = lock_record(&entry)?;
        if record.run_id.as_deref() != Some(run_id) || !record.status.is_active() {
            return Err(ProjectError::RuntimeUnavailable {
                reason: "the start reservation is no longer active".to_owned(),
            });
        }
        let runtime_revision = next_runtime_revision(&record)?;
        record.pid = Some(pid);
        record.control = Some(control);
        record.runtime_revision = runtime_revision;
        let should_stop = record.stop_requested;
        let state = snapshot(key, &record);
        entry.changed.notify_all();
        drop(record);
        drop(lifecycle);
        Ok((should_stop, state))
    }

    pub(super) fn resume_and_mark_running(
        &self,
        key: &ServiceKey,
        run_id: &str,
        process: &mut SpawnedProcess,
    ) -> Result<ResumeDisposition, String> {
        let lifecycle = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| "runtime Start lifecycle synchronization failed".to_owned())?;
        let entry = self
            .existing_entry(key)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the start reservation no longer exists".to_owned())?;
        let mut record = lock_record(&entry).map_err(|error| error.to_string())?;
        if record.run_id.as_deref() != Some(run_id) || !record.status.is_active() {
            return Err("the start reservation is no longer active".to_owned());
        }
        if lifecycle.shutting_down || record.stop_requested {
            let runtime_revision = if record.status != ServiceRuntimeStatus::Stopping {
                Some(next_runtime_revision(&record).map_err(|error| error.to_string())?)
            } else {
                None
            };
            if let Some(revision) = runtime_revision {
                record.status = ServiceRuntimeStatus::Stopping;
                record.runtime_revision = revision;
            }
            record.stop_requested = true;
            entry.changed.notify_all();
            return Ok(ResumeDisposition::StopRequested);
        }
        if record.status != ServiceRuntimeStatus::Starting {
            return Err(format!(
                "the process cannot be resumed from runtime state {}",
                record.status.as_str()
            ));
        }

        let runtime_revision = next_runtime_revision(&record).map_err(|error| error.to_string())?;
        process
            .resume()
            .map_err(|error| format!("Windows could not resume the assigned process: {error}"))?;
        record.status = ServiceRuntimeStatus::Running;
        record.started_at = Some(current_timestamp());
        record.runtime_revision = runtime_revision;
        let state = snapshot(key, &record);
        entry.changed.notify_all();
        drop(record);
        drop(lifecycle);
        self.emit_runtime_snapshot(state.clone());
        Ok(ResumeDisposition::Running(state))
    }

    pub(super) fn fail_start(&self, key: &ServiceKey, run_id: &str, mut reason: String) -> String {
        let entry = match self.existing_entry(key) {
            Ok(Some(entry)) => entry,
            Ok(None) => return reason,
            Err(error) => {
                eprintln!("Server Dashboard runtime error: {error}");
                return reason;
            }
        };
        let (mut state, control) = {
            let mut record = match lock_record(&entry) {
                Ok(record) => record,
                Err(error) => {
                    eprintln!("Server Dashboard runtime error: {error}");
                    return reason;
                }
            };
            if record.run_id.as_deref() != Some(run_id) || !record.status.is_active() {
                return reason;
            }
            let runtime_revision = match next_runtime_revision(&record) {
                Ok(revision) => revision,
                Err(error) => {
                    eprintln!("Server Dashboard runtime error: {error}");
                    return reason;
                }
            };
            record.status = ServiceRuntimeStatus::Failed;
            record.pid = None;
            record.exit_code = None;
            record.error = Some(reason.clone());
            record.stop_requested = false;
            let control = record.control.take();
            record.runtime_revision = runtime_revision;
            let state = snapshot(key, &record);
            entry.changed.notify_all();
            (state, control)
        };

        if let Some(control) = control {
            if let Err(error) = control.terminate_close_and_wait(CLOSE_WAIT.as_millis() as u32) {
                reason = format!("{reason}; process cleanup also failed: {error}");
                if let Ok(mut record) = lock_record(&entry) {
                    if record.run_id.as_deref() == Some(run_id)
                        && record.status == ServiceRuntimeStatus::Failed
                    {
                        if let Ok(runtime_revision) = next_runtime_revision(&record) {
                            record.error = Some(reason.clone());
                            record.runtime_revision = runtime_revision;
                            state = snapshot(key, &record);
                        }
                    }
                }
            }
        }
        self.emit_runtime_snapshot(state);
        reason
    }

    pub(super) fn finish_run(&self, key: &ServiceKey, run_id: &str, result: Result<u32, String>) {
        let entry = match self.existing_entry(key) {
            Ok(Some(entry)) => entry,
            Ok(None) => return,
            Err(error) => {
                eprintln!("Server Dashboard runtime error: {error}");
                return;
            }
        };
        let (state, control) = {
            let mut record = match lock_record(&entry) {
                Ok(record) => record,
                Err(error) => {
                    eprintln!("Server Dashboard runtime error: {error}");
                    return;
                }
            };
            if record.run_id.as_deref() != Some(run_id) || !record.status.is_active() {
                return;
            }
            let runtime_revision = match next_runtime_revision(&record) {
                Ok(revision) => revision,
                Err(error) => {
                    eprintln!("Server Dashboard runtime error: {error}");
                    return;
                }
            };

            match result {
                Ok(code) => {
                    record.exit_code = Some(code);
                    if record.stop_requested {
                        record.status = ServiceRuntimeStatus::Stopped;
                        record.error = None;
                    } else if code == 0 {
                        record.status = ServiceRuntimeStatus::Exited;
                    } else {
                        record.status = ServiceRuntimeStatus::Failed;
                        record.error = Some(format!("The process exited with code {code}."));
                    }
                }
                Err(reason) => {
                    record.status = ServiceRuntimeStatus::Failed;
                    record.exit_code = None;
                    record.error = Some(reason);
                }
            }
            record.pid = None;
            record.stop_requested = false;
            let control = record.control.take();
            record.runtime_revision = runtime_revision;
            let state = snapshot(key, &record);
            entry.changed.notify_all();
            (state, control)
        };

        if let Some(control) = control {
            if let Err(error) = control.close_job() {
                eprintln!("Server Dashboard could not close a service Job handle: {error}");
            }
        }
        self.emit_runtime_snapshot(state);
    }

    pub(super) fn set_runtime_error(&self, key: &ServiceKey, run_id: &str, reason: String) {
        let entry = match self.existing_entry(key) {
            Ok(Some(entry)) => entry,
            _ => return,
        };
        let state = {
            let mut record = match lock_record(&entry) {
                Ok(record) => record,
                Err(_) => return,
            };
            if record.run_id.as_deref() != Some(run_id) {
                return;
            }
            if record.error.as_deref() == Some(reason.as_str()) {
                return;
            }
            let Ok(runtime_revision) = next_runtime_revision(&record) else {
                return;
            };
            record.error = Some(reason);
            record.runtime_revision = runtime_revision;
            snapshot(key, &record)
        };
        self.emit_runtime_snapshot(state);
    }

    pub(super) fn get_runtime_for_key(
        &self,
        key: &ServiceKey,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        let Some(entry) = self.existing_entry(key)? else {
            return Ok(ServiceRuntimeSnapshot::stopped(
                &key.project_id,
                &key.service_id,
            ));
        };
        let record = lock_record(&entry)?;
        Ok(snapshot(key, &record))
    }
}

fn current_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
