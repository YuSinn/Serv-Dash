use crate::projects::ProjectError;
use crate::runtime::model::{LogSource, ServiceRuntimeStatus, MAX_LOG_ENTRIES};
use crate::runtime::test_support::{
    assert_process_terminated, long_command, manager, open_process, powershell_command,
    process_is_running, read_pid, wait_for_status, wait_until, Setup, TEST_TIMEOUT,
};
use serde_json::Value;
use std::fs;
use std::sync::{Arc, Barrier};
use uuid::Uuid;

#[test]
fn concurrent_second_start_is_rejected() {
    let setup = Setup::new("concurrent-start", long_command(), ".");
    let launch = setup.launch();
    let (manager, _) = manager();
    let barrier = Arc::new(Barrier::new(3));

    let first = {
        let manager = manager.clone();
        let launch = launch.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            manager.start(launch)
        })
    };
    let second = {
        let manager = manager.clone();
        let launch = launch.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            manager.start(launch)
        })
    };
    barrier.wait();

    let results = [
        first.join().expect("first start thread should finish"),
        second.join().expect("second start thread should finish"),
    ];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ProjectError::ServiceAlreadyActive { .. })))
            .count(),
        1
    );

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("winning process should stop");
}

#[test]
fn runtime_transitions_from_stopped_through_starting_to_running() {
    let setup = Setup::new("runtime-transitions", long_command(), ".");
    let (manager, emitter) = manager();
    assert_eq!(
        manager
            .get_runtime(&setup.project_id, &setup.service_id)
            .expect("initial runtime should be available")
            .status,
        ServiceRuntimeStatus::Stopped
    );

    let running = manager
        .start(setup.launch())
        .expect("long-running process should start");
    assert_eq!(running.status, ServiceRuntimeStatus::Running);
    assert!(running.pid.is_some());
    assert!(running.started_at.is_some());

    let statuses = emitter.statuses();
    let starting = statuses
        .iter()
        .position(|status| *status == ServiceRuntimeStatus::Starting)
        .expect("starting should be emitted");
    let running_index = statuses
        .iter()
        .position(|status| *status == ServiceRuntimeStatus::Running)
        .expect("running should be emitted");
    assert!(starting < running_index);

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process should stop");
}

#[test]
fn zero_exit_code_becomes_exited() {
    let setup = Setup::new("zero-exit", "exit /b 0", ".");
    let (manager, _) = manager();
    manager
        .start(setup.launch())
        .expect("successful command should start");

    let exited = wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    assert_eq!(exited.exit_code, Some(0));
    assert_eq!(exited.pid, None);
}

#[test]
fn nonzero_exit_code_becomes_failed() {
    let setup = Setup::new("nonzero-exit", "exit /b 7", ".");
    let (manager, _) = manager();
    manager
        .start(setup.launch())
        .expect("failing command should still be created");

    let failed = wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Failed,
    );
    assert_eq!(failed.exit_code, Some(7));
    assert_eq!(failed.pid, None);
    assert!(failed
        .error
        .as_deref()
        .is_some_and(|error| error.contains('7')));
}

#[test]
fn stdout_and_stderr_are_captured_separately() {
    let command = "echo out-1& echo out-2& echo err-1 1>&2& echo err-2 1>&2";
    let setup = Setup::new("separate-streams", command, ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("command should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );

    assert!(wait_until(TEST_TIMEOUT, || {
        manager
            .get_logs(&setup.project_id, &setup.service_id)
            .expect("logs should be available")
            .entries
            .len()
            >= 4
    }));
    let logs = manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("logs should be available");
    let stdout = logs
        .entries
        .iter()
        .filter(|entry| entry.source == LogSource::Stdout)
        .map(|entry| entry.text.trim_end())
        .collect::<Vec<_>>();
    let stderr = logs
        .entries
        .iter()
        .filter(|entry| entry.source == LogSource::Stderr)
        .map(|entry| entry.text.trim_end())
        .collect::<Vec<_>>();
    assert_eq!(stdout, ["out-1", "out-2"]);
    assert_eq!(stderr, ["err-1", "err-2"]);
}

#[test]
fn invalid_utf8_output_is_decoded_lossily() {
    let setup = Setup::new("invalid-utf8", "exit /b 0", ".");
    let launch = setup.set_command(&powershell_command(
        "[Console]::OpenStandardOutput().WriteByte(255)",
    ));
    let (manager, _) = manager();
    manager
        .start(launch)
        .expect("PowerShell fixture should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );

    assert!(wait_until(TEST_TIMEOUT, || {
        manager
            .get_logs(&setup.project_id, &setup.service_id)
            .expect("logs should be available")
            .entries
            .iter()
            .any(|entry| entry.text.contains('\u{fffd}'))
    }));
}

#[test]
fn ring_buffer_discards_the_oldest_entries() {
    let setup = Setup::new(
        "ring-buffer",
        "for /L %i in (1,1,2105) do @echo line-%i",
        ".",
    );
    let (manager, _) = manager();
    manager
        .start(setup.launch())
        .expect("loop command should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );

    assert!(wait_until(TEST_TIMEOUT, || {
        manager
            .get_logs(&setup.project_id, &setup.service_id)
            .expect("logs should be available")
            .entries
            .last()
            .is_some_and(|entry| entry.text == "line-2105")
    }));
    let logs = manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("logs should be available");
    assert_eq!(logs.entries.len(), MAX_LOG_ENTRIES);
    assert!(logs.entries[0].sequence > 1);
    assert_eq!(
        logs.entries.last().map(|entry| entry.text.trim_end()),
        Some("line-2105")
    );
}

#[test]
fn logs_can_be_cleared() {
    let setup = Setup::new("clear-logs", "echo retained-until-cleared", ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("command should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    assert!(wait_until(TEST_TIMEOUT, || {
        !manager
            .get_logs(&setup.project_id, &setup.service_id)
            .expect("logs should be available")
            .entries
            .is_empty()
    }));

    let cleared = manager
        .clear_logs(&setup.project_id, &setup.service_id)
        .expect("logs should clear");
    assert!(cleared.entries.is_empty());
    assert!(manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("logs should remain available")
        .entries
        .is_empty());
}

#[test]
fn stop_is_idempotent_and_keeps_logs() {
    let setup = Setup::new(
        "idempotent-stop",
        "echo before-stop& ping.exe -n 60 127.0.0.1 >nul",
        ".",
    );
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("process should start");
    assert!(wait_until(TEST_TIMEOUT, || {
        !manager
            .get_logs(&setup.project_id, &setup.service_id)
            .expect("logs should be available")
            .entries
            .is_empty()
    }));

    let first = manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("first stop should succeed");
    let second = manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("second stop should be idempotent");
    assert_eq!(first.status, ServiceRuntimeStatus::Stopped);
    assert_eq!(second.status, ServiceRuntimeStatus::Stopped);
    assert!(!manager
        .get_logs(&setup.project_id, &setup.service_id)
        .expect("logs should remain after stop")
        .entries
        .is_empty());
}

#[test]
fn active_runtime_blocks_service_and_project_mutations() {
    let setup = Setup::new("mutation-guard", long_command(), ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("process should start");

    assert!(matches!(
        manager.ensure_service_inactive(&setup.project_id, &setup.service_id, "edit"),
        Err(ProjectError::ServiceRuntimeActive { .. })
    ));
    assert!(matches!(
        manager.ensure_service_inactive(&setup.project_id, &setup.service_id, "remove"),
        Err(ProjectError::ServiceRuntimeActive { .. })
    ));
    assert!(matches!(
        manager.ensure_project_inactive(&setup.project_id, "remove"),
        Err(ProjectError::ProjectRuntimeActive { .. })
    ));

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process should stop");
    assert!(manager
        .ensure_service_inactive(&setup.project_id, &setup.service_id, "edit")
        .is_ok());
}

#[test]
fn spontaneous_exit_releases_runtime_control() {
    let setup = Setup::new("release-control", "exit /b 0", ".");
    let (manager, _) = manager();
    let launch = setup.launch();
    manager
        .start(launch.clone())
        .expect("first command should start");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
    assert!(!manager
        .has_control(&setup.project_id, &setup.service_id)
        .expect("control state should be readable"));

    manager.start(launch).expect("service should start again");
    wait_for_status(
        &manager,
        &setup.project_id,
        &setup.service_id,
        ServiceRuntimeStatus::Exited,
    );
}

#[test]
fn stop_terminates_the_main_process() {
    let setup = Setup::new("terminate-main", long_command(), ".");
    let (manager, _) = manager();
    let running = manager.start(setup.launch()).expect("process should start");
    let process = open_process(running.pid.expect("running process should have a PID"));
    assert!(process_is_running(&process));

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process should stop");
    assert_process_terminated(&process);
}

#[test]
fn stop_terminates_a_descendant_process() {
    let setup = Setup::new("terminate-child", "exit /b 0", ".");
    let launch = setup.set_command(&powershell_command(
        "$p = Start-Process -FilePath 'ping.exe' -ArgumentList '-n','60','127.0.0.1' -WindowStyle Hidden -PassThru\n$p.Id | Set-Content -LiteralPath 'child.pid' -Encoding ascii\nWait-Process -Id $p.Id",
    ));
    let (manager, _) = manager();
    let running = manager.start(launch).expect("process tree should start");
    let parent = open_process(running.pid.expect("main process should have a PID"));
    let child = open_process(read_pid(&setup.root.join("child.pid")));
    assert!(process_is_running(&parent));
    assert!(process_is_running(&child));

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process tree should stop");
    assert_process_terminated(&parent);
    assert_process_terminated(&child);
}

#[test]
fn dropping_the_manager_terminates_the_entire_tree() {
    let setup = Setup::new("drop-manager", "exit /b 0", ".");
    let launch = setup.set_command(&powershell_command(
        "$p = Start-Process -FilePath 'ping.exe' -ArgumentList '-n','60','127.0.0.1' -WindowStyle Hidden -PassThru\n$p.Id | Set-Content -LiteralPath 'child.pid' -Encoding ascii\nWait-Process -Id $p.Id",
    ));
    let (manager, emitter) = manager();
    let running = manager.start(launch).expect("process tree should start");
    let parent = open_process(running.pid.expect("main process should have a PID"));
    let child = open_process(read_pid(&setup.root.join("child.pid")));

    drop(manager);
    drop(emitter);
    assert_process_terminated(&parent);
    assert_process_terminated(&child);
}

#[test]
fn start_and_stop_do_not_change_persisted_json() {
    let setup = Setup::new("no-runtime-persistence", long_command(), ".");
    let before = fs::read(&setup.data_file).expect("projects JSON should exist");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("process should start");
    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process should stop");
    let after = fs::read(&setup.data_file).expect("projects JSON should still exist");

    assert_eq!(after, before);
    let data: Value = serde_json::from_slice(&after).expect("projects JSON should parse");
    assert_eq!(data["version"], 2);
    for forbidden in ["pid", "runId", "startedAt", "exitCode", "logs", "runtime"] {
        assert!(
            !contains_key(&data, forbidden),
            "projects JSON must not contain the key {forbidden}"
        );
    }
}

#[test]
fn unrelated_service_key_stays_stopped() {
    let setup = Setup::new("unrelated-runtime", long_command(), ".");
    let (manager, _) = manager();
    manager.start(setup.launch()).expect("process should start");

    let other = manager
        .get_runtime(&setup.project_id, &Uuid::new_v4().to_string())
        .expect("unrelated state should be available");
    assert_eq!(other.status, ServiceRuntimeStatus::Stopped);

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("process should stop");
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
