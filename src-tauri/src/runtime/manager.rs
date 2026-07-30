use crate::projects::{ProjectError, ServiceLaunchSpec};
use crate::runtime::emitter::{RuntimeEventEmitter, TauriRuntimeEventEmitter};
use crate::runtime::io_tasks::{spawn_log_reader, spawn_process_monitor};
use crate::runtime::model::{
    ServiceLogEntry, ServiceLogsSnapshot, ServiceRuntimeSnapshot, ServiceRuntimeStatus,
};
use crate::runtime::windows_process::{self, RunControl, SpawnedProcess};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use uuid::Uuid;

pub(super) const STOP_WAIT: Duration = Duration::from_secs(5);
pub(super) const CLOSE_WAIT: Duration = Duration::from_secs(2);
const START_SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ServiceKey {
    pub(super) project_id: String,
    pub(super) service_id: String,
}

impl ServiceKey {
    pub(super) fn new(project_id: &str, service_id: &str) -> Self {
        Self {
            project_id: project_id.to_owned(),
            service_id: service_id.to_owned(),
        }
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct RuntimeRecordInspection {
    pub(crate) runtime: ServiceRuntimeSnapshot,
    pub(crate) logs: ServiceLogsSnapshot,
    pub(crate) log_bytes: usize,
    pub(crate) next_sequence: u64,
    pub(crate) stop_requested: bool,
    pub(crate) has_control: bool,
}

pub(super) struct RuntimeRecord {
    pub(super) run_id: Option<String>,
    pub(super) status: ServiceRuntimeStatus,
    pub(super) pid: Option<u32>,
    pub(super) started_at: Option<String>,
    pub(super) exit_code: Option<u32>,
    pub(super) error: Option<String>,
    pub(super) stop_requested: bool,
    pub(super) control: Option<Arc<RunControl>>,
    pub(super) logs: VecDeque<ServiceLogEntry>,
    pub(super) log_bytes: usize,
    pub(super) next_sequence: u64,
    pub(super) runtime_revision: u64,
    pub(super) logs_revision: u64,
}

impl Default for RuntimeRecord {
    fn default() -> Self {
        Self {
            run_id: None,
            status: ServiceRuntimeStatus::Stopped,
            pid: None,
            started_at: None,
            exit_code: None,
            error: None,
            stop_requested: false,
            control: None,
            logs: VecDeque::new(),
            log_bytes: 0,
            next_sequence: 1,
            runtime_revision: 0,
            logs_revision: 0,
        }
    }
}

pub(super) struct RuntimeEntry {
    pub(super) record: Mutex<RuntimeRecord>,
    pub(super) changed: Condvar,
}

impl RuntimeEntry {
    fn new() -> Self {
        Self {
            record: Mutex::new(RuntimeRecord::default()),
            changed: Condvar::new(),
        }
    }
}

pub(super) struct StartLifecycle {
    pub(super) shutting_down: bool,
    pub(super) in_flight: usize,
}

pub(super) struct RuntimeInner {
    pub(super) entries: Mutex<HashMap<ServiceKey, Arc<RuntimeEntry>>>,
    pub(super) lifecycle: Mutex<StartLifecycle>,
    pub(super) starts_finished: Condvar,
    pub(super) shutting_down: AtomicBool,
    pub(super) emitter: Arc<dyn RuntimeEventEmitter>,
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown_internal() {
            eprintln!("Server Dashboard shutdown cleanup failed: {error}");
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeManager {
    pub(super) inner: Arc<RuntimeInner>,
}

struct StartActivity {
    inner: Arc<RuntimeInner>,
}

impl Drop for StartActivity {
    fn drop(&mut self) {
        match self.inner.lifecycle.lock() {
            Ok(mut lifecycle) => {
                debug_assert!(lifecycle.in_flight > 0);
                lifecycle.in_flight = lifecycle.in_flight.saturating_sub(1);
                self.inner.starts_finished.notify_all();
            }
            Err(_) => {
                eprintln!("Server Dashboard could not release an in-flight Start guard.");
            }
        }
    }
}

pub(crate) struct StartReservation {
    manager: RuntimeManager,
    pub(super) key: ServiceKey,
    pub(super) run_id: String,
    armed: bool,
    _activity: StartActivity,
}

impl StartReservation {
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StartReservation {
    fn drop(&mut self) {
        if self.armed {
            self.manager.fail_start(
                &self.key,
                &self.run_id,
                "Process start was cancelled before creation completed.".to_owned(),
            );
        }
    }
}

impl RuntimeManager {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self::with_emitter(Arc::new(TauriRuntimeEventEmitter::new(app)))
    }

    pub(crate) fn with_emitter(emitter: Arc<dyn RuntimeEventEmitter>) -> Self {
        Self {
            inner: Arc::new(RuntimeInner {
                entries: Mutex::new(HashMap::new()),
                lifecycle: Mutex::new(StartLifecycle {
                    shutting_down: false,
                    in_flight: 0,
                }),
                starts_finished: Condvar::new(),
                shutting_down: AtomicBool::new(false),
                emitter,
            }),
        }
    }

    pub(crate) fn reserve_start(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<StartReservation, ProjectError> {
        let activity = self.begin_start()?;
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let run_id = Uuid::new_v4().to_string();
        let (runtime_snapshot, logs_snapshot) = {
            let mut record = lock_record(&entry)?;
            if record.status.is_active() {
                return Err(ProjectError::ServiceAlreadyActive {
                    project_id: project_id.to_owned(),
                    service_id: service_id.to_owned(),
                    status: record.status.as_str().to_owned(),
                });
            }

            let runtime_revision = next_runtime_revision(&record)?;
            let logs_revision = next_logs_revision(&record)?;

            record.run_id = Some(run_id.clone());
            record.status = ServiceRuntimeStatus::Starting;
            record.pid = None;
            record.started_at = None;
            record.exit_code = None;
            record.error = None;
            record.stop_requested = false;
            record.control = None;
            record.logs.clear();
            record.log_bytes = 0;
            record.next_sequence = 1;
            record.runtime_revision = runtime_revision;
            record.logs_revision = logs_revision;
            let runtime_snapshot = snapshot(&key, &record);
            let logs_snapshot = logs_snapshot(&key, &record);
            entry.changed.notify_all();
            (runtime_snapshot, logs_snapshot)
        };
        self.emit_runtime_snapshot(runtime_snapshot);
        self.emit_logs_cleared_snapshot(logs_snapshot);

        Ok(StartReservation {
            manager: self.clone(),
            key,
            run_id,
            armed: true,
            _activity: activity,
        })
    }

    #[cfg(test)]
    pub(crate) fn start(
        &self,
        launch: ServiceLaunchSpec,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        let reservation = self.reserve_start(&launch.project_id, &launch.service_id)?;
        self.launch_reserved(launch, reservation)
    }

    pub(crate) fn launch_reserved(
        &self,
        launch: ServiceLaunchSpec,
        mut reservation: StartReservation,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        if launch.project_id != reservation.key.project_id
            || launch.service_id != reservation.key.service_id
        {
            let reason = "The reserved service does not match the validated launch configuration."
                .to_owned();
            self.fail_start(&reservation.key, &reservation.run_id, reason.clone());
            reservation.disarm();
            return Err(ProjectError::ProcessStart { reason });
        }

        if self.inner.shutting_down.load(Ordering::Acquire) {
            self.fail_start(
                &reservation.key,
                &reservation.run_id,
                "Server Dashboard is closing and cannot start another process.".to_owned(),
            );
            reservation.disarm();
            return Err(ProjectError::RuntimeShuttingDown);
        }

        let mut process =
            match windows_process::spawn_command(&launch.command, &launch.working_directory, || {
                self.inner.shutting_down.load(Ordering::Acquire)
            }) {
                Ok(process) => process,
                Err(error) => {
                    let reason =
                        format!("Windows could not create the configured process: {error}");
                    let reason = self.fail_start(&reservation.key, &reservation.run_id, reason);
                    reservation.disarm();
                    return Err(ProjectError::ProcessStart { reason });
                }
            };

        let control = process.control();
        let (stop_requested, installed) = match self.install_control(
            &reservation.key,
            &reservation.run_id,
            process.pid,
            Arc::clone(&control),
        ) {
            Ok(result) => result,
            Err(error) => {
                return self.abort_spawned_start(&mut reservation, &mut process, error.to_string());
            }
        };
        self.emit_runtime_snapshot(installed);

        let weak = Arc::downgrade(&self.inner);
        if let Err(error) = spawn_log_reader(
            Weak::clone(&weak),
            reservation.key.clone(),
            reservation.run_id.clone(),
            crate::runtime::model::LogSource::Stdout,
            process.take_stdout(),
        ) {
            return self.abort_spawned_start(
                &mut reservation,
                &mut process,
                format!("The stdout reader could not be started: {error}"),
            );
        }
        if let Err(error) = spawn_log_reader(
            Weak::clone(&weak),
            reservation.key.clone(),
            reservation.run_id.clone(),
            crate::runtime::model::LogSource::Stderr,
            process.take_stderr(),
        ) {
            return self.abort_spawned_start(
                &mut reservation,
                &mut process,
                format!("The stderr reader could not be started: {error}"),
            );
        }
        if let Err(error) = spawn_process_monitor(
            weak,
            reservation.key.clone(),
            reservation.run_id.clone(),
            process.take_wait_process(),
        ) {
            return self.abort_spawned_start(
                &mut reservation,
                &mut process,
                format!("The process monitor could not be started: {error}"),
            );
        }

        #[cfg(test)]
        if let Err(error) = process.pause_before_resume() {
            return self.abort_spawned_start(
                &mut reservation,
                &mut process,
                format!("The test pause before ResumeThread failed: {error}"),
            );
        }

        if stop_requested {
            return self.stop_spawned_start(&mut reservation, &mut process);
        }

        match self.resume_and_mark_running(&reservation.key, &reservation.run_id, &mut process) {
            Ok(crate::runtime::state::ResumeDisposition::Running(snapshot)) => {
                reservation.disarm();
                Ok(snapshot)
            }
            Ok(crate::runtime::state::ResumeDisposition::StopRequested) => {
                self.stop_spawned_start(&mut reservation, &mut process)
            }
            Err(reason) => self.abort_spawned_start(&mut reservation, &mut process, reason),
        }
    }

    pub(crate) fn stop(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let Some(entry) = self.existing_entry(&key)? else {
            return Ok(ServiceRuntimeSnapshot::stopped(project_id, service_id));
        };

        let (run_id, initial_control, changed_snapshot) = {
            let mut record = lock_record(&entry)?;
            if !record.status.is_active() {
                return Ok(snapshot(&key, &record));
            }

            let changed = record.status != ServiceRuntimeStatus::Stopping;
            let runtime_revision = changed
                .then(|| next_runtime_revision(&record))
                .transpose()?;
            let run_id = record
                .run_id
                .clone()
                .ok_or_else(|| ProjectError::RuntimeUnavailable {
                    reason: "an active service has no run identifier".to_owned(),
                })?;
            record.status = ServiceRuntimeStatus::Stopping;
            record.stop_requested = true;
            if let Some(revision) = runtime_revision {
                record.runtime_revision = revision;
            }
            let control = record.control.clone();
            let state = changed.then(|| snapshot(&key, &record));
            entry.changed.notify_all();
            (run_id, control, state)
        };
        if let Some(state) = changed_snapshot {
            self.emit_runtime_snapshot(state);
        }

        let control = match initial_control {
            Some(control) => control,
            None => match wait_for_control(&entry, &run_id, STOP_WAIT)? {
                Some(control) => control,
                None => return self.get_runtime_for_key(&key),
            },
        };

        if let Err(error) = control.terminate() {
            if !control.wait(0).map_err(process_stop_error)? {
                let reason = format!("Windows could not terminate the service Job: {error}");
                self.set_runtime_error(&key, &run_id, reason.clone());
                return Err(ProjectError::ProcessStop { reason });
            }
        }

        let completed = control
            .wait(STOP_WAIT.as_millis() as u32)
            .map_err(process_stop_error)?;
        if !completed {
            control.close_job().map_err(process_stop_error)?;
            if !control
                .wait(CLOSE_WAIT.as_millis() as u32)
                .map_err(process_stop_error)?
            {
                let reason =
                    "The service process did not terminate before the stop timeout.".to_owned();
                self.set_runtime_error(&key, &run_id, reason.clone());
                return Err(ProjectError::ProcessStop { reason });
            }
        }

        let code = control.exit_code().map_err(process_stop_error)?;
        self.finish_run(&key, &run_id, Ok(code));
        self.get_runtime_for_key(&key)
    }

    pub(crate) fn shutdown(&self) -> Result<(), ProjectError> {
        self.inner
            .shutdown_internal()
            .map_err(|reason| ProjectError::RuntimeUnavailable { reason })
    }

    pub(super) fn entry(&self, key: &ServiceKey) -> Result<Arc<RuntimeEntry>, ProjectError> {
        let mut entries = self
            .inner
            .entries
            .lock()
            .map_err(|_| runtime_lock_error())?;
        Ok(Arc::clone(
            entries
                .entry(key.clone())
                .or_insert_with(|| Arc::new(RuntimeEntry::new())),
        ))
    }

    pub(super) fn existing_entry(
        &self,
        key: &ServiceKey,
    ) -> Result<Option<Arc<RuntimeEntry>>, ProjectError> {
        Ok(self
            .inner
            .entries
            .lock()
            .map_err(|_| runtime_lock_error())?
            .get(key)
            .cloned())
    }

    pub(super) fn emit_runtime_snapshot(&self, snapshot: ServiceRuntimeSnapshot) {
        if let Err(error) = self.inner.emitter.emit_runtime(&snapshot) {
            if !self.inner.shutting_down.load(Ordering::Acquire) {
                eprintln!("Server Dashboard could not emit a runtime update: {error}");
            }
        }
    }

    pub(super) fn emit_logs_cleared_snapshot(&self, snapshot: ServiceLogsSnapshot) {
        if let Err(error) = self.inner.emitter.emit_logs_cleared(&snapshot) {
            if !self.inner.shutting_down.load(Ordering::Acquire) {
                eprintln!("Server Dashboard could not emit a log-clear update: {error}");
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.inner.shutting_down.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn inspect_record_for_test(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<RuntimeRecordInspection, ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let record = lock_record(&entry)?;
        Ok(RuntimeRecordInspection {
            runtime: snapshot(&key, &record),
            logs: logs_snapshot(&key, &record),
            log_bytes: record.log_bytes,
            next_sequence: record.next_sequence,
            stop_requested: record.stop_requested,
            has_control: record.control.is_some(),
        })
    }

    #[cfg(test)]
    pub(crate) fn seed_logs_for_test(
        &self,
        project_id: &str,
        service_id: &str,
        run_id: Option<String>,
        entries: Vec<ServiceLogEntry>,
    ) -> Result<(), ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let mut record = lock_record(&entry)?;
        record.run_id = run_id;
        record.log_bytes = entries.iter().map(|entry| entry.text.len()).sum();
        record.next_sequence = entries
            .last()
            .map_or(1, |entry| entry.sequence.saturating_add(1));
        record.logs = entries.into();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn seed_runtime_for_test(
        &self,
        project_id: &str,
        service_id: &str,
        run_id: Option<String>,
        status: ServiceRuntimeStatus,
        error: Option<String>,
        stop_requested: bool,
    ) -> Result<(), ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let mut record = lock_record(&entry)?;
        record.run_id = run_id;
        record.status = status;
        record.pid = None;
        record.started_at = None;
        record.exit_code = None;
        record.error = error;
        record.stop_requested = stop_requested;
        record.control = None;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_status_preserving_control_for_test(
        &self,
        project_id: &str,
        service_id: &str,
        status: ServiceRuntimeStatus,
        stop_requested: bool,
    ) -> Result<(), ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let mut record = lock_record(&entry)?;
        record.status = status;
        record.stop_requested = stop_requested;
        if !status.is_active() {
            record.pid = None;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn append_log_for_test(
        &self,
        key: &ServiceKey,
        run_id: &str,
        source: crate::runtime::model::LogSource,
        bytes: &[u8],
    ) -> Result<(), ProjectError> {
        self.inner.append_log(key, run_id, source, bytes)
    }

    #[cfg(test)]
    pub(super) fn report_reader_error_for_test(
        &self,
        key: &ServiceKey,
        run_id: &str,
        source: crate::runtime::model::LogSource,
        error: &str,
    ) {
        self.inner.report_reader_error(key, run_id, source, error);
    }

    #[cfg(test)]
    pub(crate) fn set_revisions_for_test(
        &self,
        project_id: &str,
        service_id: &str,
        runtime_revision: u64,
        logs_revision: u64,
    ) -> Result<(), ProjectError> {
        let key = ServiceKey::new(project_id, service_id);
        let entry = self.entry(&key)?;
        let mut record = lock_record(&entry)?;
        record.runtime_revision = runtime_revision;
        record.logs_revision = logs_revision;
        Ok(())
    }

    fn begin_start(&self) -> Result<StartActivity, ProjectError> {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| runtime_lock_error())?;
        if lifecycle.shutting_down {
            return Err(ProjectError::RuntimeShuttingDown);
        }
        lifecycle.in_flight =
            lifecycle
                .in_flight
                .checked_add(1)
                .ok_or_else(|| ProjectError::RuntimeUnavailable {
                    reason: "the in-flight Start counter overflowed".to_owned(),
                })?;
        drop(lifecycle);
        Ok(StartActivity {
            inner: Arc::clone(&self.inner),
        })
    }

    fn abort_spawned_start(
        &self,
        reservation: &mut StartReservation,
        process: &mut SpawnedProcess,
        reason: String,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        let reason = match process.terminate_before_running() {
            Ok(()) => reason,
            Err(cleanup_error) => {
                format!("{reason}; process cleanup also failed: {cleanup_error}")
            }
        };
        let reason = self.fail_start(&reservation.key, &reservation.run_id, reason);
        reservation.disarm();
        Err(ProjectError::ProcessStart { reason })
    }

    fn stop_spawned_start(
        &self,
        reservation: &mut StartReservation,
        process: &mut SpawnedProcess,
    ) -> Result<ServiceRuntimeSnapshot, ProjectError> {
        let control = process.control();
        if let Err(error) = process.terminate_before_running() {
            return self.abort_spawned_start(
                reservation,
                process,
                format!("The suspended process could not be stopped: {error}"),
            );
        }
        let code = control.exit_code().map_err(process_stop_error)?;
        self.finish_run(&reservation.key, &reservation.run_id, Ok(code));
        reservation.disarm();
        self.get_runtime_for_key(&reservation.key)
    }
}

impl RuntimeInner {
    pub(super) fn shutdown_internal(&self) -> Result<(), String> {
        {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .map_err(|_| "runtime Start lifecycle synchronization failed".to_owned())?;
            lifecycle.shutting_down = true;
            self.shutting_down.store(true, Ordering::Release);
        }

        let mut failures = Vec::new();
        let deadline = Instant::now() + START_SHUTDOWN_WAIT;
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "runtime Start lifecycle synchronization failed".to_owned())?;
        while lifecycle.in_flight != 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                failures.push(format!(
                    "{} Start operation(s) remained in flight after {} ms",
                    lifecycle.in_flight,
                    START_SHUTDOWN_WAIT.as_millis()
                ));
                break;
            };
            let (next, wait) = self
                .starts_finished
                .wait_timeout(lifecycle, remaining)
                .map_err(|_| "runtime Start lifecycle synchronization failed".to_owned())?;
            lifecycle = next;
            if wait.timed_out() && lifecycle.in_flight != 0 {
                failures.push(format!(
                    "{} Start operation(s) remained in flight after {} ms",
                    lifecycle.in_flight,
                    START_SHUTDOWN_WAIT.as_millis()
                ));
                break;
            }
        }
        drop(lifecycle);

        let entries = self
            .entries
            .lock()
            .map_err(|_| "the runtime entry map could not be locked during shutdown".to_owned())?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut controls = Vec::new();
        for entry in entries {
            let mut record = match entry.record.lock() {
                Ok(record) => record,
                Err(_) => {
                    failures
                        .push("runtime record synchronization failed during shutdown".to_owned());
                    continue;
                }
            };
            if let Some(control) = record.control.clone() {
                push_unique_control(&mut controls, control);
            }

            if record.status.is_active() {
                let runtime_revision = if record.status != ServiceRuntimeStatus::Stopping {
                    match next_runtime_revision_string(&record) {
                        Ok(revision) => Some(revision),
                        Err(error) => {
                            failures.push(error);
                            continue;
                        }
                    }
                } else {
                    None
                };
                record.status = ServiceRuntimeStatus::Stopping;
                record.stop_requested = true;
                if let Some(revision) = runtime_revision {
                    record.runtime_revision = revision;
                }
                entry.changed.notify_all();
            }
        }

        for control in controls {
            if let Err(error) = control.terminate_close_and_wait(CLOSE_WAIT.as_millis() as u32) {
                failures.push(format!("service Job cleanup failed: {error}"));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

fn push_unique_control(controls: &mut Vec<Arc<RunControl>>, control: Arc<RunControl>) {
    if !controls
        .iter()
        .any(|existing| Arc::ptr_eq(existing, &control))
    {
        controls.push(control);
    }
}

pub(super) fn snapshot(key: &ServiceKey, record: &RuntimeRecord) -> ServiceRuntimeSnapshot {
    ServiceRuntimeSnapshot {
        project_id: key.project_id.clone(),
        service_id: key.service_id.clone(),
        run_id: record.run_id.clone(),
        runtime_revision: record.runtime_revision,
        status: record.status,
        pid: record.pid.filter(|_| record.status.is_active()),
        started_at: record.started_at.clone(),
        exit_code: record.exit_code,
        error: record.error.clone(),
    }
}

pub(super) fn logs_snapshot(key: &ServiceKey, record: &RuntimeRecord) -> ServiceLogsSnapshot {
    ServiceLogsSnapshot {
        project_id: key.project_id.clone(),
        service_id: key.service_id.clone(),
        run_id: record.run_id.clone(),
        logs_revision: record.logs_revision,
        entries: record.logs.iter().cloned().collect(),
    }
}

pub(super) fn next_runtime_revision(record: &RuntimeRecord) -> Result<u64, ProjectError> {
    record
        .runtime_revision
        .checked_add(1)
        .ok_or_else(|| ProjectError::RuntimeUnavailable {
            reason: "runtime revision overflowed".to_owned(),
        })
}

pub(super) fn next_logs_revision(record: &RuntimeRecord) -> Result<u64, ProjectError> {
    record
        .logs_revision
        .checked_add(1)
        .ok_or_else(|| ProjectError::RuntimeUnavailable {
            reason: "logs revision overflowed".to_owned(),
        })
}

fn next_runtime_revision_string(record: &RuntimeRecord) -> Result<u64, String> {
    record
        .runtime_revision
        .checked_add(1)
        .ok_or_else(|| "runtime revision overflowed".to_owned())
}

pub(super) fn lock_record(
    entry: &RuntimeEntry,
) -> Result<MutexGuard<'_, RuntimeRecord>, ProjectError> {
    entry.record.lock().map_err(|_| runtime_lock_error())
}

pub(super) fn runtime_lock_error() -> ProjectError {
    ProjectError::RuntimeUnavailable {
        reason: "runtime state synchronization failed".to_owned(),
    }
}

fn process_stop_error(error: io::Error) -> ProjectError {
    ProjectError::ProcessStop {
        reason: error.to_string(),
    }
}

fn wait_for_control(
    entry: &Arc<RuntimeEntry>,
    run_id: &str,
    timeout: Duration,
) -> Result<Option<Arc<RunControl>>, ProjectError> {
    let deadline = Instant::now() + timeout;
    let mut record = lock_record(entry)?;
    loop {
        if record.run_id.as_deref() != Some(run_id) || !record.status.is_active() {
            return Ok(None);
        }
        if let Some(control) = record.control.clone() {
            return Ok(Some(control));
        }

        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err(ProjectError::ProcessStop {
                reason: "The process was still being initialized when Stop timed out.".to_owned(),
            });
        };
        let (next_record, wait_result) = entry
            .changed
            .wait_timeout(record, remaining)
            .map_err(|_| runtime_lock_error())?;
        record = next_record;
        if wait_result.timed_out() && record.control.is_none() {
            return Err(ProjectError::ProcessStop {
                reason: "The process was still being initialized when Stop timed out.".to_owned(),
            });
        }
    }
}
