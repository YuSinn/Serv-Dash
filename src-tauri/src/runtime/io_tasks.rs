use crate::projects::ProjectError;
use crate::runtime::manager::{
    lock_record, next_logs_revision, next_runtime_revision, runtime_lock_error, snapshot,
    RuntimeInner, RuntimeManager, ServiceKey,
};
use crate::runtime::model::{
    LogSource, ServiceLogEntry, ServiceLogEvent, MAX_LOG_BUFFER_BYTES, MAX_LOG_ENTRIES,
    MAX_LOG_ENTRY_BYTES,
};
use crate::runtime::windows_process;
use chrono::{SecondsFormat, Utc};
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::io::OwnedHandle;
use std::sync::atomic::Ordering;
use std::sync::Weak;

pub(super) fn spawn_log_reader(
    runtime: Weak<RuntimeInner>,
    key: ServiceKey,
    run_id: String,
    source: LogSource,
    mut file: File,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("service-{}-reader", source.as_str()))
        .spawn(move || {
            let mut read_buffer = [0_u8; 4_096];
            let mut line = Vec::with_capacity(4_096);
            let mut line_was_chunked = false;
            loop {
                let count = match file.read(&mut read_buffer) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) if error.kind() == io::ErrorKind::BrokenPipe => break,
                    Err(error) => {
                        if let Some(runtime) = runtime.upgrade() {
                            runtime.report_reader_error(&key, &run_id, source, &error.to_string());
                        }
                        return;
                    }
                };

                for byte in &read_buffer[..count] {
                    if *byte == b'\n' {
                        if line.last() == Some(&b'\r') {
                            line.pop();
                        }
                        if !line.is_empty() || !line_was_chunked {
                            append_log_bytes(&runtime, &key, &run_id, source, &line);
                        }
                        line.clear();
                        line_was_chunked = false;
                    } else {
                        line.push(*byte);
                        if line.len() == MAX_LOG_ENTRY_BYTES {
                            append_log_bytes(&runtime, &key, &run_id, source, &line);
                            line.clear();
                            line_was_chunked = true;
                        }
                    }
                }
            }

            if !line.is_empty() {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                append_log_bytes(&runtime, &key, &run_id, source, &line);
            }
        })
        .map(|_| ())
}

pub(super) fn spawn_process_monitor(
    runtime: Weak<RuntimeInner>,
    key: ServiceKey,
    run_id: String,
    process: OwnedHandle,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("service-process-monitor".to_owned())
        .spawn(move || {
            let result = windows_process::wait_process(&process).map_err(|error| {
                format!("Windows could not determine the process exit status: {error}")
            });
            if let Some(runtime) = runtime.upgrade() {
                RuntimeManager { inner: runtime }.finish_run(&key, &run_id, result);
            }
        })
        .map(|_| ())
}

impl RuntimeInner {
    pub(super) fn append_log(
        &self,
        key: &ServiceKey,
        run_id: &str,
        source: LogSource,
        bytes: &[u8],
    ) -> Result<(), ProjectError> {
        let entry = {
            let entries = self.entries.lock().map_err(|_| runtime_lock_error())?;
            entries.get(key).cloned()
        };
        let Some(entry) = entry else {
            return Ok(());
        };

        let text = String::from_utf8_lossy(bytes)
            .chars()
            .take(MAX_LOG_ENTRY_BYTES)
            .collect::<String>();
        let event = {
            let mut record = lock_record(&entry)?;
            if record.run_id.as_deref() != Some(run_id) {
                return Ok(());
            }
            let logs_revision = next_logs_revision(&record)?;
            let log = ServiceLogEntry {
                sequence: record.next_sequence,
                timestamp: current_timestamp(),
                source,
                text,
            };
            record.next_sequence = record.next_sequence.saturating_add(1);
            let entry_bytes = log.text.len();
            while record.logs.len() >= MAX_LOG_ENTRIES
                || record.log_bytes.saturating_add(entry_bytes) > MAX_LOG_BUFFER_BYTES
            {
                let Some(removed) = record.logs.pop_front() else {
                    break;
                };
                record.log_bytes = record.log_bytes.saturating_sub(removed.text.len());
            }
            record.log_bytes = record.log_bytes.saturating_add(entry_bytes);
            record.logs.push_back(log.clone());
            record.logs_revision = logs_revision;
            ServiceLogEvent {
                project_id: key.project_id.clone(),
                service_id: key.service_id.clone(),
                run_id: run_id.to_owned(),
                logs_revision: record.logs_revision,
                entry: log,
            }
        };

        if let Err(error) = self.emitter.emit_log(&event) {
            if !self.shutting_down.load(Ordering::Acquire) {
                eprintln!("Server Dashboard could not emit a service log entry: {error}");
            }
        }
        Ok(())
    }

    pub(super) fn report_reader_error(
        &self,
        key: &ServiceKey,
        run_id: &str,
        source: LogSource,
        error: &str,
    ) {
        let entry = match self.entries.lock() {
            Ok(entries) => entries.get(key).cloned(),
            Err(_) => None,
        };
        let Some(entry) = entry else {
            return;
        };
        let state = {
            let mut record = match entry.record.lock() {
                Ok(record) => record,
                Err(_) => return,
            };
            if record.run_id.as_deref() != Some(run_id) {
                return;
            }
            let Ok(runtime_revision) = next_runtime_revision(&record) else {
                return;
            };
            record.error = Some(format!(
                "The {} stream could not be read completely: {error}",
                source.as_str()
            ));
            record.runtime_revision = runtime_revision;
            snapshot(key, &record)
        };
        if let Err(emit_error) = self.emitter.emit_runtime(&state) {
            if !self.shutting_down.load(Ordering::Acquire) {
                eprintln!("Server Dashboard could not emit a runtime update: {emit_error}");
            }
        }
    }
}

fn append_log_bytes(
    runtime: &Weak<RuntimeInner>,
    key: &ServiceKey,
    run_id: &str,
    source: LogSource,
    bytes: &[u8],
) {
    if let Some(runtime) = runtime.upgrade() {
        if let Err(error) = runtime.append_log(key, run_id, source, bytes) {
            eprintln!("Server Dashboard could not buffer a service log entry: {error}");
        }
    }
}

fn current_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
