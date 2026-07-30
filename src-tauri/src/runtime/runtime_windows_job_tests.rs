use crate::projects::ProjectError;
use crate::runtime::model::ServiceRuntimeStatus;
use crate::runtime::test_support::{
    assert_process_terminated, long_command, manager, open_process, powershell_command,
    process_is_running, read_pid, wait_until, Setup, TempDir, TEST_TIMEOUT,
};
use crate::runtime::windows_process;
use crate::runtime::windows_process::test_api::{
    self, handle_is_inheritable, TestHooks, TestPoint,
};
use std::fs;
use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const HOST_HELPER_MODE: &str = "SERVER_DASHBOARD_JOB_HOST_HELPER_MODE";
const HOST_HELPER_ROOT: &str = "SERVER_DASHBOARD_JOB_HOST_HELPER_ROOT";
const HOST_HELPER_TEST: &str = "runtime::windows_job_tests::atomic_job_host_helper";
const ABRUPT_HOST_REPETITIONS: usize = 10;

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

#[test]
fn create_process_returns_with_atomic_job_and_two_valid_attributes() {
    let setup = Setup::new("atomic-job-attributes", long_command(), ".");
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

    let event = pause
        .wait(TEST_TIMEOUT)
        .expect("CreateProcessW return pause should be reached");
    assert_eq!(event.point, TestPoint::AfterCreateProcess);
    let pid = event.pid.expect("the created process should have a PID");
    let process = open_process(pid);
    assert!(process_is_running(&process));

    let observations = hooks.observations();
    assert_eq!(observations.attribute_count, Some(2));
    assert_eq!(observations.attribute_lists_initialized, 1);
    assert_eq!(observations.attribute_lists_deleted, 1);
    assert_eq!(observations.handle_list_updates, 1);
    assert_eq!(observations.job_list_updates, 1);
    assert_eq!(observations.create_process_calls, 1);
    assert_eq!(observations.inherited_handles.len(), 3);
    assert_eq!(observations.job_handles.len(), 1);
    assert!(observations
        .inherited_handle_flags
        .iter()
        .all(|flags| handle_is_inheritable(*flags)));
    assert!(observations
        .job_handle_flags
        .iter()
        .all(|flags| !handle_is_inheritable(*flags)));
    assert!(!observations
        .inherited_handles
        .contains(&observations.job_handles[0]));
    assert!(test_api::raw_handle_is_valid(observations.job_handles[0]));
    assert_eq!(observations.created_pids, [pid]);
    assert_eq!(observations.process_in_job, [true]);

    pause
        .release()
        .expect("CreateProcessW return pause should release");
    let running = start.finish().expect("the service should start");
    assert_eq!(running.pid, Some(pid));
    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("the service should stop");
    assert_process_terminated(&process);
}

#[test]
fn job_list_update_failure_creates_no_process_and_releases_every_handle() {
    let setup = Setup::new("job-list-update-failure", long_command(), ".");
    let mut hooks = TestHooks::new();
    hooks.inject_job_list_update_failure();
    let hooks = Arc::new(hooks);

    let result = test_api::with_hooks(Arc::clone(&hooks), || {
        windows_process::spawn_command(&setup.launch().command, &setup.root, || false)
    });
    let error = match result {
        Ok(mut process) => {
            process
                .terminate_before_running()
                .expect("unexpected process should still be cleaned");
            panic!("JOB_LIST update failure must prevent CreateProcessW")
        }
        Err(error) => error,
    };
    assert!(error.to_string().contains("PROC_THREAD_ATTRIBUTE_JOB_LIST"));
    assert!(error.to_string().contains("Windows 10 or later"));

    let observations = hooks.observations();
    assert_eq!(observations.attribute_count, Some(2));
    assert_eq!(observations.attribute_lists_initialized, 1);
    assert_eq!(observations.attribute_lists_deleted, 1);
    assert_eq!(observations.handle_list_updates, 1);
    assert_eq!(observations.job_list_updates, 0);
    assert_eq!(observations.create_process_calls, 0);
    assert!(observations.created_pids.is_empty());
    for handle in observations
        .inherited_handles
        .iter()
        .chain(observations.job_handles.iter())
    {
        assert!(
            !test_api::raw_handle_is_valid(*handle),
            "early return leaked handle {handle:#x}"
        );
    }
}

#[test]
fn normal_shutdown_during_post_create_pause_cleans_process_and_rejects_new_start() {
    let setup = Setup::new("shutdown-post-create", long_command(), ".");
    let persisted_before = fs::read(&setup.data_file).expect("projects JSON should be readable");
    let (manager, emitter) = manager();
    let mut hooks = TestHooks::new();
    let pause = hooks.add_pause(TestPoint::AfterCreateProcess);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };

    let event = pause
        .wait(TEST_TIMEOUT)
        .expect("post-create pause should be reached");
    let process = open_process(event.pid.expect("created process should have a PID"));
    let shutdown = {
        let manager = manager.clone();
        spawn_timed(move || manager.shutdown())
    };
    assert!(wait_until(TEST_TIMEOUT, || manager.is_shutting_down()));
    assert!(matches!(
        manager.start(setup.launch()),
        Err(ProjectError::RuntimeShuttingDown)
    ));

    pause.release().expect("post-create pause should release");
    assert!(start.finish().is_err());
    shutdown
        .finish()
        .expect("shutdown should finish after the in-flight Start exits");
    assert_process_terminated(&process);
    assert!(!emitter.statuses().contains(&ServiceRuntimeStatus::Running));
    assert_eq!(
        fs::read(&setup.data_file).expect("projects JSON should remain readable"),
        persisted_before
    );
}

#[test]
fn shutdown_before_create_process_prevents_process_creation() {
    let setup = Setup::new("shutdown-before-create", long_command(), ".");
    let (manager, emitter) = manager();
    let mut hooks = TestHooks::new();
    let pause = hooks.add_pause(TestPoint::BeforeCreateProcess);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        let hooks = Arc::clone(&hooks);
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };

    pause
        .wait(TEST_TIMEOUT)
        .expect("pre-create pause should be reached");
    let shutdown = {
        let manager = manager.clone();
        spawn_timed(move || manager.shutdown())
    };
    assert!(wait_until(TEST_TIMEOUT, || manager.is_shutting_down()));
    pause.release().expect("pre-create pause should release");

    assert!(start.finish().is_err());
    shutdown.finish().expect("shutdown should finish");
    let observations = hooks.observations();
    assert_eq!(observations.create_process_calls, 0);
    assert!(observations.created_pids.is_empty());
    assert_eq!(observations.attribute_lists_initialized, 1);
    assert_eq!(observations.attribute_lists_deleted, 1);
    assert!(!emitter.statuses().contains(&ServiceRuntimeStatus::Running));
}

#[test]
fn shutdown_during_create_process_cleans_atomically_assigned_process() {
    let setup = Setup::new("shutdown-during-create", long_command(), ".");
    let (manager, emitter) = manager();
    let mut hooks = TestHooks::new();
    let entered = hooks.add_pause(TestPoint::CreateProcessEntered);
    let created = hooks.add_pause(TestPoint::AfterCreateProcess);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        let hooks = Arc::clone(&hooks);
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };

    entered
        .wait(TEST_TIMEOUT)
        .expect("CreateProcessW entry seam should be reached");
    let shutdown = {
        let manager = manager.clone();
        spawn_timed(move || manager.shutdown())
    };
    assert!(wait_until(TEST_TIMEOUT, || manager.is_shutting_down()));
    entered
        .release()
        .expect("CreateProcessW entry seam should release");

    let event = created
        .wait(TEST_TIMEOUT)
        .expect("CreateProcessW should return after shutdown begins");
    let process = open_process(event.pid.expect("created process should have a PID"));
    assert_eq!(hooks.observations().process_in_job, [true]);
    created.release().expect("post-create pause should release");

    assert!(start.finish().is_err());
    shutdown.finish().expect("shutdown should finish");
    assert_process_terminated(&process);
    assert!(!emitter.statuses().contains(&ServiceRuntimeStatus::Running));
}

#[test]
fn shutdown_after_create_before_resume_never_marks_process_running() {
    let setup = Setup::new("shutdown-before-resume", long_command(), ".");
    let (manager, emitter) = manager();
    let mut hooks = TestHooks::new();
    let pause = hooks.add_pause(TestPoint::BeforeResume);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };

    let event = pause
        .wait(TEST_TIMEOUT)
        .expect("pre-resume pause should be reached");
    let process = open_process(event.pid.expect("created process should have a PID"));
    let shutdown = {
        let manager = manager.clone();
        spawn_timed(move || manager.shutdown())
    };
    assert!(wait_until(TEST_TIMEOUT, || manager.is_shutting_down()));
    pause.release().expect("pre-resume pause should release");

    let _ = start.finish();
    shutdown.finish().expect("shutdown should finish");
    assert_process_terminated(&process);
    assert!(!emitter.statuses().contains(&ServiceRuntimeStatus::Running));
}

#[test]
fn resume_failure_terminates_process_and_closes_job() {
    let setup = Setup::new("resume-failure-cleanup", long_command(), ".");
    let (manager, _) = manager();
    let mut hooks = TestHooks::new();
    hooks.inject_resume_failure();
    let pause = hooks.add_pause(TestPoint::AfterCreateProcess);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        let hooks = Arc::clone(&hooks);
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };

    let event = pause
        .wait(TEST_TIMEOUT)
        .expect("post-create pause should be reached");
    let process = open_process(event.pid.expect("created process should have a PID"));
    pause.release().expect("post-create pause should release");
    let error = start
        .finish()
        .expect_err("injected ResumeThread failure should fail Start");
    assert!(error.to_string().contains("injected ResumeThread failure"));
    assert_process_terminated(&process);
    assert_eq!(
        manager
            .get_runtime(&setup.project_id, &setup.service_id)
            .expect("runtime should remain available")
            .status,
        ServiceRuntimeStatus::Failed
    );
    assert!(!manager
        .has_control(&setup.project_id, &setup.service_id)
        .expect("control state should remain available"));
}

#[test]
fn cleanup_wait_timeout_is_reported_even_when_fallback_confirms_termination() {
    let setup = Setup::new("cleanup-timeout", long_command(), ".");
    let (manager, _) = manager();
    let mut hooks = TestHooks::new();
    hooks.inject_resume_failure();
    hooks.inject_cleanup_wait_timeouts(1);
    let pause = hooks.add_pause(TestPoint::AfterCreateProcess);
    let hooks = Arc::new(hooks);
    let start = {
        let manager = manager.clone();
        let launch = setup.launch();
        spawn_timed(move || test_api::with_hooks(hooks, || manager.start(launch)))
    };

    let event = pause
        .wait(TEST_TIMEOUT)
        .expect("post-create pause should be reached");
    let process = open_process(event.pid.expect("created process should have a PID"));
    pause.release().expect("post-create pause should release");
    let error = start
        .finish()
        .expect_err("cleanup timeout must keep Start unsuccessful");
    let message = error.to_string();
    assert!(message.contains("injected ResumeThread failure"));
    assert!(message.contains("timed out"));
    assert_process_terminated(&process);
    assert_eq!(
        manager
            .get_runtime(&setup.project_id, &setup.service_id)
            .expect("runtime should remain available")
            .status,
        ServiceRuntimeStatus::Failed
    );
}

#[test]
fn stop_terminates_parent_child_and_grandchild() {
    let setup = Setup::new("stop-three-generations", "exit /b 0", ".");
    let launch = setup.set_command(&three_generation_command());
    let (manager, _) = manager();
    let running = manager.start(launch).expect("process tree should start");
    let parent_pid = running.pid.expect("cmd.exe should have a PID");
    let child_pid = read_pid(&setup.root.join("child.pid"));
    let grandchild_pid = read_pid(&setup.root.join("grandchild.pid"));
    let parent = open_process(parent_pid);
    let child = open_process(child_pid);
    let grandchild = open_process(grandchild_pid);
    assert!(process_is_running(&parent));
    assert!(process_is_running(&child));
    assert!(process_is_running(&grandchild));

    manager
        .stop(&setup.project_id, &setup.service_id)
        .expect("three-generation process tree should stop");
    assert_process_terminated(&parent);
    assert_process_terminated(&child);
    assert_process_terminated(&grandchild);
}

#[test]
fn abrupt_host_exit_after_create_kills_suspended_process_repeatedly() {
    for iteration in 0..ABRUPT_HOST_REPETITIONS {
        let temp = TempDir::new(&format!("abrupt-host-suspended-{iteration}"));
        let root = temp.0.clone();
        let mut host = spawn_host_helper("suspended", &root);
        let host_pid = host.id();
        let process_pid = read_pid(&root.join("created.pid"));
        let process = open_process(process_pid);
        assert!(process_is_running(&process));

        terminate_host(&mut host);
        assert_process_terminated(&process);
        println!(
            "atomic-host-iteration={iteration} host_pid={host_pid} process_pid={process_pid} process_handle={:#x}",
            process.as_raw_handle() as usize
        );
        drop(process);
        drop(temp);
        assert!(!root.exists(), "host fixture should be removed");
    }
}

#[test]
fn abrupt_host_exit_kills_parent_child_and_grandchild() {
    let temp = TempDir::new("abrupt-host-three-generations");
    let root = temp.0.clone();
    let mut host = spawn_host_helper("tree", &root);
    let host_pid = host.id();
    let parent_pid = read_pid(&root.join("parent.pid"));
    let child_pid = read_pid(&root.join("child.pid"));
    let grandchild_pid = read_pid(&root.join("grandchild.pid"));
    let parent = open_process(parent_pid);
    let child = open_process(child_pid);
    let grandchild = open_process(grandchild_pid);
    assert!(process_is_running(&parent));
    assert!(process_is_running(&child));
    assert!(process_is_running(&grandchild));

    terminate_host(&mut host);
    assert_process_terminated(&parent);
    assert_process_terminated(&child);
    assert_process_terminated(&grandchild);
    println!(
        "tree-host_pid={host_pid} parent_pid={parent_pid} child_pid={child_pid} grandchild_pid={grandchild_pid} handles={:#x},{:#x},{:#x}",
        parent.as_raw_handle() as usize,
        child.as_raw_handle() as usize,
        grandchild.as_raw_handle() as usize
    );
    drop((parent, child, grandchild));
    drop(temp);
    assert!(!root.exists(), "host fixture should be removed");
}

#[test]
fn atomic_job_host_helper() {
    let Ok(mode) = std::env::var(HOST_HELPER_MODE) else {
        return;
    };
    let root = std::env::var_os(HOST_HELPER_ROOT)
        .map(std::path::PathBuf::from)
        .expect("host helper root should be provided");

    match mode.as_str() {
        "suspended" => run_suspended_host_helper(&root),
        "tree" => run_tree_host_helper(&root),
        other => panic!("unknown host helper mode {other}"),
    }
}

fn run_suspended_host_helper(root: &std::path::Path) {
    let mut hooks = TestHooks::new();
    let pause = hooks.add_pause(TestPoint::AfterCreateProcess);
    let hooks = Arc::new(hooks);
    let pid_file = root.join("created.pid");
    let observer = std::thread::spawn(move || {
        let event = pause
            .wait(TEST_TIMEOUT)
            .expect("host helper should reach post-create pause");
        fs::write(
            pid_file,
            event
                .pid
                .expect("host helper process should have a PID")
                .to_string(),
        )
        .expect("host helper PID should be written");
        let (_sender, receiver) = mpsc::channel::<()>();
        let _ = receiver.recv_timeout(Duration::from_secs(30));
        drop(pause);
    });

    let result = test_api::with_hooks(hooks, || {
        windows_process::spawn_command(long_command(), root, || false)
    });
    let _process = result.expect("host helper process should be created");
    observer.join().expect("host helper observer should finish");
}

fn run_tree_host_helper(root: &std::path::Path) {
    let command = three_generation_command();
    let mut process = windows_process::spawn_command(&command, root, || false)
        .expect("host helper tree should be created");
    fs::write(root.join("parent.pid"), process.pid.to_string())
        .expect("host helper parent PID should be written");
    process
        .resume()
        .expect("host helper tree should be resumed");
    let _ = read_pid(&root.join("child.pid"));
    let _ = read_pid(&root.join("grandchild.pid"));
    let (_sender, receiver) = mpsc::channel::<()>();
    let _ = receiver.recv_timeout(Duration::from_secs(30));
    drop(process);
}

fn spawn_host_helper(mode: &str, root: &std::path::Path) -> Child {
    Command::new(std::env::current_exe().expect("test executable path should be available"))
        .arg("--exact")
        .arg(HOST_HELPER_TEST)
        .arg("--nocapture")
        .env(HOST_HELPER_MODE, mode)
        .env(HOST_HELPER_ROOT, root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("host helper test process should start")
}

fn terminate_host(host: &mut Child) {
    let host_handle = open_process(host.id());
    assert!(process_is_running(&host_handle));
    host.kill().expect("host helper should terminate abruptly");
    assert_process_terminated(&host_handle);
    assert!(wait_until(TEST_TIMEOUT, || host
        .try_wait()
        .expect("host helper status should be readable")
        .is_some()));
    host.wait().expect("host helper should be reaped");
}

fn three_generation_command() -> String {
    powershell_command(
        "$PID | Set-Content -LiteralPath 'child.pid' -Encoding ascii\n$p = Start-Process -FilePath 'ping.exe' -ArgumentList '-n','60','127.0.0.1' -WindowStyle Hidden -PassThru\n$p.Id | Set-Content -LiteralPath 'grandchild.pid' -Encoding ascii\nWait-Process -Id $p.Id",
    )
}
