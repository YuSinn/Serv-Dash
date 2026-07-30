use crate::projects::{ProjectStore, ServiceInput, ServiceLaunchSpec};
use crate::runtime::emitter::RuntimeEventEmitter;
use crate::runtime::manager::RuntimeManager;
use crate::runtime::model::{
    ServiceLogEvent, ServiceLogsSnapshot, ServiceRuntimeSnapshot, ServiceRuntimeStatus,
};
use std::fs;
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(8);
const FIXTURE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct TempDir(pub(super) PathBuf);

impl TempDir {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "server-dashboard-runtime-{name}-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("temporary runtime directory should be created");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let deadline = Instant::now() + FIXTURE_CLEANUP_TIMEOUT;
        loop {
            match fs::remove_dir_all(&self.0) {
                Ok(()) => return,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return,
                Err(error) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                    drop(error);
                }
                Err(error) => {
                    eprintln!(
                        "could not remove runtime test directory {}: {error}",
                        self.0.display()
                    );
                    return;
                }
            }
        }
    }
}

pub(super) struct Setup {
    pub(super) _temp: TempDir,
    pub(super) store: ProjectStore,
    pub(super) project_id: String,
    pub(super) service_id: String,
    pub(super) root: PathBuf,
    pub(super) data_file: PathBuf,
}

impl Setup {
    pub(super) fn new(name: &str, command: &str, working_directory: &str) -> Self {
        let temp = TempDir::new(name);
        let root = temp.0.join("project");
        fs::create_dir(&root).expect("test project root should be created");
        if working_directory != "." {
            fs::create_dir_all(root.join(working_directory))
                .expect("test working directory should be created");
        }
        let data_file = temp.0.join("data").join("projects.json");
        let store = ProjectStore::new(data_file.clone());
        let project = store
            .add_project(root.to_string_lossy().as_ref(), Some("Runtime project"))
            .expect("test project should be added")
            .remove(0);
        let service = store
            .add_service(
                &project.id,
                &service_input("Runtime service", working_directory, command),
            )
            .expect("test service should be added")
            .remove(0);

        Self {
            _temp: temp,
            store,
            project_id: project.id,
            service_id: service.id,
            root,
            data_file,
        }
    }

    pub(super) fn launch(&self) -> ServiceLaunchSpec {
        self.store
            .prepare_service_start(&self.project_id, &self.service_id)
            .expect("launch configuration should be valid")
    }

    pub(super) fn set_command(&self, command: &str) -> ServiceLaunchSpec {
        self.set_service(".", command)
    }

    pub(super) fn set_service(&self, working_directory: &str, command: &str) -> ServiceLaunchSpec {
        self.store
            .update_service(
                &self.project_id,
                &self.service_id,
                &service_input("Runtime service", working_directory, command),
            )
            .expect("test service should be updated");
        self.launch()
    }
}

pub(super) fn service_input(name: &str, working_directory: &str, command: &str) -> ServiceInput {
    ServiceInput {
        name: name.to_owned(),
        working_directory: working_directory.to_owned(),
        command: command.to_owned(),
        expected_port: None,
        local_url: None,
    }
}

#[derive(Default)]
pub(super) struct RecordingEmitter {
    snapshots: Mutex<Vec<ServiceRuntimeSnapshot>>,
    logs: Mutex<Vec<ServiceLogEvent>>,
    cleared: Mutex<Vec<ServiceLogsSnapshot>>,
}

impl RecordingEmitter {
    pub(super) fn statuses(&self) -> Vec<ServiceRuntimeStatus> {
        self.snapshots
            .lock()
            .expect("recorded statuses should be available")
            .iter()
            .map(|snapshot| snapshot.status)
            .collect()
    }

    pub(super) fn logs(&self) -> Vec<ServiceLogEvent> {
        self.logs
            .lock()
            .expect("recorded logs should be available")
            .clone()
    }

    pub(super) fn cleared(&self) -> Vec<ServiceLogsSnapshot> {
        self.cleared
            .lock()
            .expect("recorded clears should be available")
            .clone()
    }
}

impl RuntimeEventEmitter for RecordingEmitter {
    fn emit_runtime(&self, snapshot: &ServiceRuntimeSnapshot) -> Result<(), String> {
        self.snapshots
            .lock()
            .map_err(|_| "recorded status lock was poisoned".to_owned())?
            .push(snapshot.clone());
        Ok(())
    }

    fn emit_log(&self, event: &ServiceLogEvent) -> Result<(), String> {
        self.logs
            .lock()
            .map_err(|_| "recorded log lock was poisoned".to_owned())?
            .push(event.clone());
        Ok(())
    }

    fn emit_logs_cleared(&self, snapshot: &ServiceLogsSnapshot) -> Result<(), String> {
        self.cleared
            .lock()
            .map_err(|_| "recorded clear lock was poisoned".to_owned())?
            .push(snapshot.clone());
        Ok(())
    }
}

pub(super) fn manager() -> (RuntimeManager, Arc<RecordingEmitter>) {
    let emitter = Arc::new(RecordingEmitter::default());
    let manager = RuntimeManager::with_emitter(emitter.clone());
    (manager, emitter)
}

pub(super) fn long_command() -> &'static str {
    "ping.exe -n 60 127.0.0.1 >nul"
}

pub(super) fn powershell_command(script: &str) -> String {
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        base64(&bytes)
    )
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied();
        let third = chunk.get(2).copied();
        encoded.push(ALPHABET[(first >> 2) as usize] as char);
        encoded.push(ALPHABET[(((first & 0b11) << 4) | second.unwrap_or(0) >> 4) as usize] as char);
        match second {
            Some(second) => encoded.push(
                ALPHABET[(((second & 0b1111) << 2) | third.unwrap_or(0) >> 6) as usize] as char,
            ),
            None => encoded.push('='),
        }
        match third {
            Some(third) => encoded.push(ALPHABET[(third & 0b11_1111) as usize] as char),
            None => encoded.push('='),
        }
    }
    encoded
}

pub(super) fn wait_for_status(
    manager: &RuntimeManager,
    project_id: &str,
    service_id: &str,
    expected: ServiceRuntimeStatus,
) -> ServiceRuntimeSnapshot {
    let mut latest = manager
        .get_runtime(project_id, service_id)
        .expect("runtime state should be available");
    let reached = wait_until(TEST_TIMEOUT, || {
        latest = manager
            .get_runtime(project_id, service_id)
            .expect("runtime state should remain available");
        latest.status == expected
    });
    assert!(
        reached,
        "runtime did not reach {expected:?}; latest state was {:?}",
        latest.status
    );
    latest
}

pub(super) fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

pub(super) fn open_process(pid: u32) -> OwnedHandle {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!handle.is_null(), "test process {pid} should still exist");
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

pub(super) fn assert_process_terminated(handle: &OwnedHandle) {
    let result = unsafe { WaitForSingleObject(handle.as_raw_handle(), 5_000) };
    assert_eq!(result, WAIT_OBJECT_0, "process handle should be signaled");
}

pub(super) fn process_is_running(handle: &OwnedHandle) -> bool {
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => true,
        WAIT_OBJECT_0 => false,
        result => panic!("unexpected process wait result {result}"),
    }
}

pub(super) fn read_pid(path: &Path) -> u32 {
    let mut pid = None;
    let ready = wait_until(TEST_TIMEOUT, || {
        pid = fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        pid.is_some()
    });
    assert!(ready, "child PID file should become readable");
    pid.expect("child PID should be numeric")
}

use std::os::windows::io::AsRawHandle;
