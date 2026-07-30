use crate::runtime::manager::{RuntimeManager, ServiceKey};
use crate::runtime::model::{LogSource, ServiceLogEntry, ServiceRuntimeStatus};
use crate::runtime::test_support::{
    assert_process_terminated, long_command, manager, open_process, powershell_command,
    process_is_running, read_pid, wait_for_status, wait_until, Setup, TEST_TIMEOUT,
};
use crate::runtime::windows_process::test_api::{self, TestHooks, TestPoint};
use serde_json::Value;
use std::fs;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Barrier};
use std::thread::JoinHandle;

#[test]
fn runtime_revision_increases_for_observable_transitions_and_not_idempotent_stop() {
    let setup = Setup::new("runtime-revisions", long_command(), ".");
    let (manager, _) = manager();
    let initial = manager
        .get_runtime(&setup.project_id, &setup.service_id)
        .expect("initial runtime should be readable");
    assert_eq!(initial.runtime_revision, 0);

    let running = manager.start(setup.launch()).expect("process should start");
    assert!(running.runtime_revision > initial.runtime_revision);
    let stopped = manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process should stop");
    assert!(stopped.runtime_revision > running.runtime_revision);
    let idempotent = manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("second stop should be idempotent");
    assert_eq!(idempotent.runtime_revision, stopped.runtime_revision);
}

#[test]
fn revisions_continue_between_runs_and_old_terminal_run_is_ignored() {
    let setup = Setup::new("revision-runs", "exit /b 0", ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("run A should start");
    let exited_a = wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    let run_a = exited_a.run_id.clone().expect("run A id should exist");

    let launch_b = setup.set_command(long_command());
    let running_b = manager.start(launch_b).expect("run B should start");
    assert!(running_b.runtime_revision > exited_a.runtime_revision);
    assert_ne!(running_b.run_id.as_deref(), Some(run_a.as_str()));

    RuntimeManager::finish_run(
        &manager,
        &ServiceKey::new(&setup.project_id, &setup.service_id),
        &run_a,
        Ok(0),
    );
    let after_old_terminal = manager
        .get_runtime(&setup.project_id, &setup.service_id)
        .expect("runtime should remain readable");
    assert_eq!(after_old_terminal.run_id, running_b.run_id);
    assert_eq!(after_old_terminal.status, ServiceRuntimeStatus::Running);

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("run B should stop");
}

#[test]
fn concurrent_runtime_mutations_do_not_share_revisions() {
    let setup = Setup::new("concurrent-revisions", long_command(), ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("process should start");
    let barrier = Arc::new(Barrier::new(3));
    let first = {
        let manager = manager.clone();
        let project_id = setup.project_id.clone();
        let service_id = setup.service_id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            manager.stop(&project_id, &service_id)
        })
    };
    let second = {
        let manager = manager.clone();
        let project_id = setup.project_id.clone();
        let service_id = setup.service_id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            manager.stop(&project_id, &service_id)
        })
    };
    barrier.wait();
    let first = first.join().expect("first stop thread should finish");
    let second = second.join().expect("second stop thread should finish");
    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(
        first.expect("first stop should succeed").runtime_revision,
        second.expect("second stop should succeed").runtime_revision
    );
}

#[test]
fn logs_revision_orders_append_clear_snapshot_and_new_start_reset() {
    let setup = Setup::new("logs-revisions", "echo run-a", ".");
    let (manager, emitter) = manager();
    let initial = manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("initial logs should be readable");
    assert_eq!(initial.logs_revision, 0);

    manager.start(setup.launch()).expect("run A should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    assert!(wait_until(TEST_TIMEOUT, || !emitter.logs().is_empty()));
    let appended = emitter
        .logs()
        .last()
        .expect("append should be recorded")
        .clone();
    let with_log = manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("logs should be readable");
    assert_eq!(with_log.logs_revision, appended.logs_revision);
    assert!(with_log.logs_revision > initial.logs_revision);

    let stale_snapshot = with_log.clone();
    let cleared = manager
        .clear_logs(&setup.project_id, &setup.service_id)
        .expect("logs should clear");
    assert!(cleared.entries.is_empty());
    assert!(cleared.logs_revision > appended.logs_revision);
    assert!(stale_snapshot.logs_revision < cleared.logs_revision);
    assert!(appended.logs_revision < cleared.logs_revision);

    let launch_b = setup.set_command("echo run-b");
    manager.start(launch_b).expect("run B should start");
    let reset = emitter
        .cleared()
        .last()
        .expect("start reset should emit clear")
        .clone();
    assert!(reset.logs_revision > cleared.logs_revision);
    assert_ne!(reset.run_id, cleared.run_id);
}

#[test]
fn old_run_log_writers_are_rejected_by_run_id_guard() {
    let setup = Setup::new("old-writer", "echo run-a", ".");
    let (manager, emitter) = manager();
    manager.start(setup.launch()).expect("run A should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    assert!(wait_until(TEST_TIMEOUT, || !emitter.logs().is_empty()));
    let run_a_log_count = emitter.logs().len();

    let launch_b = setup.set_command("echo run-b");
    manager.start(launch_b).expect("run B should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    assert!(wait_until(TEST_TIMEOUT, || emitter.logs().len() > run_a_log_count));
    let logs = manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("logs should be readable");
    assert!(logs.entries.iter().all(|entry| entry.text == "run-b"));
}

#[test]
fn revisions_are_not_persisted() {
    let setup = Setup::new("revision-persistence", "echo persisted", ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("process should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    manager
        .clear_logs(&setup.project_id, &setup.service_id)
        .expect("logs should clear");

    let data: Value =
        serde_json::from_slice(&fs::read(&setup.data_file).expect("projects JSON should exist"))
            .expect("projects JSON should parse");
    assert!(!contains_key(&data, "runtimeRevision"));
    assert!(!contains_key(&data, "logsRevision"));
}

#[test]
fn revision_overflow_is_reported_without_wrapping() {
    let setup = Setup::new("revision-overflow", long_command(), ".");
    let (manager, _) = manager();
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("test revisions should be set");
    assert!(manager
        .reserve_start(&setup.project_id, &setup.service_id)
        .is_err());

    let other = Setup::new("logs-revision-overflow", long_command(), ".");
    manager
        .set_revisions_for_test(&other.project_id, &other.service_id, 0, u64::MAX)
        .expect("test logs revision should be set");
    assert!(manager
        .clear_logs(&other.project_id, &other.service_id)
        .is_err());
}

fn contains_key(value: &Value, requested: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(requested)
                || object.values().any(|value| contains_key(value, requested))
        }
        Value::Array(values) => values.iter().any(|value| contains_key(value, requested)),
        _ => false,
    }
}

struct TimedTask<T> {
    result: Receiver<T>,
    thread: JoinHandle<()>,
}

impl<T> TimedTask<T> {
    fn finish(self) -> T {
        let result = self
            .result
            .recv_timeout(TEST_TIMEOUT)
            .expect("background test task should finish before its timeout");
        self.thread
            .join()
            .expect("background test task should not panic");
        result
    }
}

fn spawn_timed<T: Send + 'static>(action: impl FnOnce() -> T + Send + 'static) -> TimedTask<T> {
    let (sender, result) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let value = action();
        sender
            .send(value)
            .expect("test task result receiver should remain available");
    });
    TimedTask { result, thread }
}

fn log_entry(sequence: u64, text: &str) -> ServiceLogEntry {
    ServiceLogEntry {
        sequence,
        timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
        source: LogSource::Stdout,
        text: text.to_owned(),
    }
}

#[test]
fn clear_logs_overflow_preserves_existing_entries_and_bytes() {
    let setup = Setup::new("clear-overflow", long_command(), ".");
    let (manager, _) = manager();
    let entries = vec![log_entry(1, "first"), log_entry(2, "second")];
    manager
        .seed_logs_for_test(
            &setup.project_id,
            &setup.service_id,
            Some("run".to_owned()),
            entries.clone(),
        )
        .expect("logs should seed");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, 0, u64::MAX)
        .expect("revisions should set");
    let before = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");

    assert!(manager
        .clear_logs(&setup.project_id, &setup.service_id)
        .is_err());

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.logs.entries, before.logs.entries);
    assert_eq!(after.log_bytes, before.log_bytes);
    assert_eq!(after.logs.logs_revision, u64::MAX);
}

#[test]
fn append_log_overflow_preserves_sequence_entries_eviction_and_bytes() {
    let setup = Setup::new("append-overflow", long_command(), ".");
    let (manager, _) = manager();
    let run_id = "run".to_owned();
    let entries = vec![log_entry(1, "first"), log_entry(2, "second")];
    manager
        .seed_logs_for_test(
            &setup.project_id,
            &setup.service_id,
            Some(run_id.clone()),
            entries,
        )
        .expect("logs should seed");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, 0, u64::MAX)
        .expect("revisions should set");
    let before = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");

    assert!(manager
        .append_log_for_test(
            &ServiceKey::new(&setup.project_id, &setup.service_id),
            &run_id,
            LogSource::Stdout,
            b"third",
        )
        .is_err());

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.next_sequence, before.next_sequence);
    assert_eq!(after.logs.entries, before.logs.entries);
    assert_eq!(after.log_bytes, before.log_bytes);
    assert_eq!(after.logs.logs_revision, u64::MAX);
}

#[test]
fn install_control_overflow_does_not_install_pid_or_control() {
    let setup = Setup::new("install-control-overflow", long_command(), ".");
    let (manager, _) = manager();
    let mut hooks = TestHooks::new();
    let pause = hooks.add_pause(TestPoint::AfterCreateProcess);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        let hooks = Arc::clone(&hooks);
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };
    pause
        .wait(TEST_TIMEOUT)
        .expect("CreateProcessW return pause should be reached");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");
    pause.release().expect("start should resume");
    assert!(start.finish().is_err());

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.status, ServiceRuntimeStatus::Starting);
    assert_eq!(after.runtime.pid, None);
    assert!(!after.has_control);
    assert_eq!(after.runtime.runtime_revision, u64::MAX);
}

#[test]
fn resume_and_mark_running_overflow_preserves_status_and_started_at() {
    let setup = Setup::new("resume-overflow", long_command(), ".");
    let (manager, _) = manager();
    let mut hooks = TestHooks::new();
    let pause = hooks.add_pause(TestPoint::BeforeResume);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        let hooks = Arc::clone(&hooks);
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };
    pause
        .wait(TEST_TIMEOUT)
        .expect("before-resume pause should be reached");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");
    let before = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    pause.release().expect("resume should continue");
    assert!(start.finish().is_err());

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.status, ServiceRuntimeStatus::Starting);
    assert_eq!(after.runtime.started_at, before.runtime.started_at);
    assert_eq!(after.runtime.runtime_revision, u64::MAX);
}

#[test]
fn fail_start_overflow_preserves_status_error_pid_and_control() {
    let setup = Setup::new("fail-start-overflow", long_command(), ".");
    let (manager, _) = manager();
    let reservation = manager
        .reserve_start(&setup.project_id, &setup.service_id)
        .expect("reservation should start");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");
    let before = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");

    RuntimeManager::fail_start(
        &manager,
        &reservation.key,
        &reservation.run_id,
        "boom".to_owned(),
    );

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.status, before.runtime.status);
    assert_eq!(after.runtime.error, before.runtime.error);
    assert_eq!(after.runtime.pid, before.runtime.pid);
    assert_eq!(after.has_control, before.has_control);
}

#[test]
fn finish_run_overflow_preserves_terminal_fields_pid_and_control() {
    let setup = Setup::new("finish-run-overflow", long_command(), ".");
    let (manager, _) = manager();
    let reservation = manager
        .reserve_start(&setup.project_id, &setup.service_id)
        .expect("reservation should start");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");
    let before = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");

    RuntimeManager::finish_run(&manager, &reservation.key, &reservation.run_id, Ok(0));

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.status, before.runtime.status);
    assert_eq!(after.runtime.exit_code, before.runtime.exit_code);
    assert_eq!(after.runtime.pid, before.runtime.pid);
    assert_eq!(after.has_control, before.has_control);
}

#[test]
fn set_runtime_error_overflow_preserves_previous_error() {
    let setup = Setup::new("runtime-error-overflow", long_command(), ".");
    let (manager, _) = manager();
    let run_id = "run".to_owned();
    manager
        .seed_runtime_for_test(
            &setup.project_id,
            &setup.service_id,
            Some(run_id.clone()),
            ServiceRuntimeStatus::Running,
            Some("old".to_owned()),
            false,
        )
        .expect("runtime should seed");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");

    RuntimeManager::set_runtime_error(
        &manager,
        &ServiceKey::new(&setup.project_id, &setup.service_id),
        &run_id,
        "new".to_owned(),
    );

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.error.as_deref(), Some("old"));
}

#[test]
fn real_stop_overflow_preserves_status_and_stop_requested() {
    let setup = Setup::new("stop-overflow", long_command(), ".");
    let (manager, _) = manager();
    let _reservation = manager
        .reserve_start(&setup.project_id, &setup.service_id)
        .expect("reservation should start");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");

    assert!(manager.stop(&setup.project_id, &setup.service_id).is_err());

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.status, ServiceRuntimeStatus::Starting);
    assert!(!after.stop_requested);
}

// This unit test validates observable-state atomicity only; real process cleanup is covered below.
#[test]
fn shutdown_overflow_snapshot_preserves_failing_record_and_continues_other_active_records() {
    let failing = Setup::new("shutdown-overflow-failing", long_command(), ".");
    let other = Setup::new("shutdown-overflow-other", long_command(), ".");
    let (manager, _) = manager();
    manager
        .seed_runtime_for_test(
            &failing.project_id,
            &failing.service_id,
            Some("failing".to_owned()),
            ServiceRuntimeStatus::Running,
            None,
            false,
        )
        .expect("runtime should seed");
    manager
        .seed_runtime_for_test(
            &other.project_id,
            &other.service_id,
            Some("other".to_owned()),
            ServiceRuntimeStatus::Running,
            None,
            false,
        )
        .expect("runtime should seed");
    manager
        .set_revisions_for_test(&failing.project_id, &failing.service_id, u64::MAX, 0)
        .expect("revisions should set");

    assert!(manager.shutdown().is_err());

    let failed = manager
        .inspect_record_for_test(&failing.project_id, &failing.service_id)
        .expect("record should inspect");
    let cleaned = manager
        .inspect_record_for_test(&other.project_id, &other.service_id)
        .expect("record should inspect");
    assert_eq!(failed.runtime.status, ServiceRuntimeStatus::Running);
    assert!(!failed.stop_requested);
    assert_eq!(cleaned.runtime.status, ServiceRuntimeStatus::Stopping);
    assert!(cleaned.stop_requested);
}

// These tests use production start/shutdown paths and real process handles to validate resource cleanup.
#[test]
fn shutdown_overflow_real_process_terminates_failing_and_other_jobs() {
    let (manager, _) = manager();
    let failing = start_tree_service(&manager, "shutdown-real-overflow-failing");
    let other = start_tree_service(&manager, "shutdown-real-overflow-other");
    let failing_json_before =
        fs::read(&failing.setup.data_file).expect("failing projects JSON should read");
    let other_json_before =
        fs::read(&other.setup.data_file).expect("other projects JSON should read");
    let before = manager
        .inspect_record_for_test(&failing.setup.project_id, &failing.setup.service_id)
        .expect("failing record should inspect");
    manager
        .set_revisions_for_test(
            &failing.setup.project_id,
            &failing.setup.service_id,
            u64::MAX,
            0,
        )
        .expect("failing revision should set");

    let error = manager
        .shutdown()
        .expect_err("revision overflow should be reported");

    assert_overflow_error(&error, 1);
    let after = manager
        .inspect_record_for_test(&failing.setup.project_id, &failing.setup.service_id)
        .expect("failing record should remain inspectable");
    assert_eq!(after.runtime.status, before.runtime.status);
    assert_eq!(after.stop_requested, before.stop_requested);
    assert_eq!(after.runtime.runtime_revision, u64::MAX);
    let other_after = manager
        .inspect_record_for_test(&other.setup.project_id, &other.setup.service_id)
        .expect("other record should inspect");
    assert!(matches!(
        other_after.runtime.status,
        ServiceRuntimeStatus::Stopping | ServiceRuntimeStatus::Stopped
    ));
    assert_tree_terminated("failing-overflow", &failing);
    assert_tree_terminated("other-normal", &other);
    assert_eq!(
        fs::read(&failing.setup.data_file).expect("failing JSON should read"),
        failing_json_before
    );
    assert_eq!(
        fs::read(&other.setup.data_file).expect("other JSON should read"),
        other_json_before
    );
}

#[test]
fn shutdown_overflow_real_process_single_service_terminates_job() {
    let (manager, _) = manager();
    let service = start_tree_service(&manager, "shutdown-single-overflow");
    manager
        .set_revisions_for_test(
            &service.setup.project_id,
            &service.setup.service_id,
            u64::MAX,
            0,
        )
        .expect("revision should set");

    let error = manager.shutdown().expect_err("overflow should be returned");

    assert_overflow_error(&error, 1);
    let after = manager
        .inspect_record_for_test(&service.setup.project_id, &service.setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.status, ServiceRuntimeStatus::Running);
    assert!(!after.stop_requested);
    assert_eq!(after.runtime.runtime_revision, u64::MAX);
    assert_tree_terminated("single-overflow", &service);
}

#[test]
fn shutdown_overflow_real_process_multiple_failing_services_all_terminate() {
    let (manager, _) = manager();
    let first = start_tree_service(&manager, "shutdown-multi-overflow-a");
    let second = start_tree_service(&manager, "shutdown-multi-overflow-b");
    let third = start_tree_service(&manager, "shutdown-multi-overflow-c");
    for service in [&first, &second] {
        manager
            .set_revisions_for_test(
                &service.setup.project_id,
                &service.setup.service_id,
                u64::MAX,
                0,
            )
            .expect("revision should set");
    }

    let error = manager
        .shutdown()
        .expect_err("overflow errors should be returned");

    assert_overflow_error(&error, 2);
    assert_tree_terminated("multi-overflow-a", &first);
    assert_tree_terminated("multi-overflow-b", &second);
    assert_tree_terminated("multi-normal-c", &third);
}

#[test]
fn shutdown_collects_terminal_record_with_installed_control() {
    let (manager, _) = manager();
    let service = start_tree_service(&manager, "shutdown-terminal-control");
    manager
        .set_status_preserving_control_for_test(
            &service.setup.project_id,
            &service.setup.service_id,
            ServiceRuntimeStatus::Exited,
            false,
        )
        .expect("terminal state should set while preserving control");

    manager
        .shutdown()
        .expect("terminal control cleanup should succeed");

    assert_tree_terminated("terminal-control", &service);
}

#[test]
fn shutdown_overflow_real_process_is_idempotent_after_cleanup() {
    let (manager, _) = manager();
    let service = start_tree_service(&manager, "shutdown-double-overflow");
    manager
        .set_revisions_for_test(
            &service.setup.project_id,
            &service.setup.service_id,
            u64::MAX,
            0,
        )
        .expect("revision should set");

    let first = manager
        .shutdown()
        .expect_err("first shutdown should report overflow");
    let second = manager
        .shutdown()
        .expect_err("second shutdown should remain safe and report overflow");

    assert_overflow_error(&first, 1);
    assert_overflow_error(&second, 1);
    assert_tree_terminated("double-overflow", &service);
}

#[test]
fn drop_with_revision_overflow_real_process_still_closes_job() {
    let service = {
        let (manager, _) = manager();
        let service = start_tree_service(&manager, "drop-overflow");
        manager
            .set_revisions_for_test(
                &service.setup.project_id,
                &service.setup.service_id,
                u64::MAX,
                0,
            )
            .expect("revision should set");
        service
    };

    assert_tree_terminated("drop-overflow", &service);
}

#[test]
fn shutdown_with_spontaneous_exit_remains_bounded_and_safe() {
    let setup = Setup::new("shutdown-spontaneous-exit", "exit /b 0", ".");
    let (manager, _) = manager();
    let running = manager
        .start(setup.launch())
        .expect("short process should start");
    let process = open_process(running.pid.expect("short process PID should exist"));

    let _ = manager.shutdown();

    assert_process_terminated(&process);
}

#[test]
fn shutdown_aggregates_revision_overflow_and_cleanup_error() {
    let setup = Setup::new("shutdown-overflow-cleanup-error", long_command(), ".");
    let (manager, _) = manager();
    let running = manager.start(setup.launch()).expect("process should start");
    let process = open_process(running.pid.expect("process PID should exist"));
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revision should set");
    let hooks = Arc::new(TestHooks::new());
    hooks.inject_cleanup_wait_timeouts(1);

    let error = test_api::with_hooks(hooks, || {
        manager
            .shutdown()
            .expect_err("overflow and cleanup timeout should be returned")
    });

    assert_overflow_error(&error, 1);
    let error_message = error.to_string();
    assert!(
        error_message.contains("timed out"),
        "cleanup timeout should be preserved: {error_message}"
    );
    assert_process_terminated(&process);
}
#[test]
fn report_reader_error_overflow_preserves_observable_error() {
    let setup = Setup::new("reader-error-overflow", long_command(), ".");
    let (manager, _) = manager();
    let run_id = "run".to_owned();
    manager
        .seed_runtime_for_test(
            &setup.project_id,
            &setup.service_id,
            Some(run_id.clone()),
            ServiceRuntimeStatus::Running,
            Some("old".to_owned()),
            false,
        )
        .expect("runtime should seed");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, 0)
        .expect("revisions should set");

    manager.report_reader_error_for_test(
        &ServiceKey::new(&setup.project_id, &setup.service_id),
        &run_id,
        LogSource::Stdout,
        "broken",
    );

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime.error.as_deref(), Some("old"));
}

#[test]
fn start_reset_needing_two_revisions_preserves_both_domains_if_one_overflows() {
    let setup = Setup::new("start-two-revision-overflow", long_command(), ".");
    let (manager, _) = manager();
    let entries = vec![log_entry(1, "first")];
    manager
        .seed_logs_for_test(
            &setup.project_id,
            &setup.service_id,
            Some("old".to_owned()),
            entries,
        )
        .expect("logs should seed");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, 0, u64::MAX)
        .expect("revisions should set");
    let before = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");

    assert!(manager
        .reserve_start(&setup.project_id, &setup.service_id)
        .is_err());

    let after = manager
        .inspect_record_for_test(&setup.project_id, &setup.service_id)
        .expect("record should inspect");
    assert_eq!(after.runtime, before.runtime);
    assert_eq!(after.logs, before.logs);
    assert_eq!(after.log_bytes, before.log_bytes);
}

#[test]
fn projects_json_remains_identical_and_without_revisions_after_runtime_overflow() {
    let setup = Setup::new("overflow-persistence", long_command(), ".");
    let (manager, _) = manager();
    let before = fs::read(&setup.data_file).expect("projects JSON should exist");
    manager
        .set_revisions_for_test(&setup.project_id, &setup.service_id, u64::MAX, u64::MAX)
        .expect("revisions should set");
    assert!(manager
        .reserve_start(&setup.project_id, &setup.service_id)
        .is_err());
    let after = fs::read(&setup.data_file).expect("projects JSON should exist");
    assert_eq!(after, before);

    let data: Value = serde_json::from_slice(&after).expect("projects JSON should parse");
    assert!(!contains_key(&data, "runtimeRevision"));
    assert!(!contains_key(&data, "logsRevision"));
}

struct RealTreeService {
    setup: Setup,
    parent: OwnedHandle,
    child: OwnedHandle,
    grandchild: OwnedHandle,
}

fn start_tree_service(manager: &RuntimeManager, name: &str) -> RealTreeService {
    let setup = Setup::new(name, "exit /b 0", ".");
    let launch = setup.set_command(&three_generation_command());
    let running = manager.start(launch).expect("process tree should start");
    let parent = open_process(running.pid.expect("parent PID should exist"));
    let child = open_process(read_pid(&setup.root.join("child.pid")));
    let grandchild = open_process(read_pid(&setup.root.join("grandchild.pid")));
    assert!(process_is_running(&parent));
    assert!(process_is_running(&child));
    assert!(process_is_running(&grandchild));
    RealTreeService {
        setup,
        parent,
        child,
        grandchild,
    }
}

fn assert_tree_terminated(label: &str, service: &RealTreeService) {
    assert_process_terminated(&service.parent);
    assert_process_terminated(&service.child);
    assert_process_terminated(&service.grandchild);
    println!(
        "{label}: parent={:#x} child={:#x} grandchild={:#x} signaled",
        service.parent.as_raw_handle() as usize,
        service.child.as_raw_handle() as usize,
        service.grandchild.as_raw_handle() as usize
    );
}

fn three_generation_command() -> String {
    powershell_command(
        "$PID | Set-Content -LiteralPath 'child.pid' -Encoding ascii\n$p = Start-Process -FilePath 'ping.exe' -ArgumentList '-n','60','127.0.0.1' -WindowStyle Hidden -PassThru\n$p.Id | Set-Content -LiteralPath 'grandchild.pid' -Encoding ascii\nWait-Process -Id $p.Id",
    )
}

fn assert_overflow_error(error: &impl std::fmt::Display, expected_count: usize) {
    let message = error.to_string();
    let count = message.matches("runtime revision overflowed").count();
    assert!(
        count >= expected_count,
        "expected at least {expected_count} runtime revision overflow error(s), got {count}: {message}"
    );
}
