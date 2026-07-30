use super::{HANDLE, HANDLE_FLAG_INHERIT};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::Foundation::GetHandleInformation;

const HOOK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TestPoint {
    BeforeCreateProcess,
    CreateProcessEntered,
    AfterCreateProcess,
    BeforeResume,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TestEvent {
    pub(crate) point: TestPoint,
    pub(crate) pid: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TestObservations {
    pub(crate) attribute_count: Option<u32>,
    pub(crate) attribute_lists_initialized: usize,
    pub(crate) attribute_lists_deleted: usize,
    pub(crate) handle_list_updates: usize,
    pub(crate) job_list_updates: usize,
    pub(crate) create_process_calls: usize,
    pub(crate) inherited_handles: Vec<usize>,
    pub(crate) job_handles: Vec<usize>,
    pub(crate) inherited_handle_flags: Vec<u32>,
    pub(crate) job_handle_flags: Vec<u32>,
    pub(crate) created_pids: Vec<u32>,
    pub(crate) process_in_job: Vec<bool>,
}

struct PauseHook {
    reached: SyncSender<TestEvent>,
    release: Mutex<Receiver<()>>,
}

pub(crate) struct TestPauseControl {
    reached: Receiver<TestEvent>,
    release: SyncSender<()>,
}

impl TestPauseControl {
    pub(crate) fn wait(&self, timeout: Duration) -> io::Result<TestEvent> {
        self.reached
            .recv_timeout(timeout)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    io::Error::new(io::ErrorKind::TimedOut, "test pause was not reached")
                }
                RecvTimeoutError::Disconnected => {
                    io::Error::new(io::ErrorKind::BrokenPipe, "test pause was disconnected")
                }
            })
    }

    pub(crate) fn release(&self) -> io::Result<()> {
        self.release.send(()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test pause could not be released",
            )
        })
    }
}

pub(crate) struct TestHooks {
    pauses: HashMap<TestPoint, PauseHook>,
    fail_job_list_update: bool,
    fail_resume: bool,
    cleanup_wait_timeouts: AtomicUsize,
    observations: Mutex<TestObservations>,
}

impl TestHooks {
    pub(crate) fn new() -> Self {
        Self {
            pauses: HashMap::new(),
            fail_job_list_update: false,
            fail_resume: false,
            cleanup_wait_timeouts: AtomicUsize::new(0),
            observations: Mutex::new(TestObservations::default()),
        }
    }

    pub(crate) fn add_pause(&mut self, point: TestPoint) -> TestPauseControl {
        let (reached_tx, reached_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        self.pauses.insert(
            point,
            PauseHook {
                reached: reached_tx,
                release: Mutex::new(release_rx),
            },
        );
        TestPauseControl {
            reached: reached_rx,
            release: release_tx,
        }
    }

    pub(crate) fn inject_job_list_update_failure(&mut self) {
        self.fail_job_list_update = true;
    }

    pub(crate) fn inject_resume_failure(&mut self) {
        self.fail_resume = true;
    }

    pub(crate) fn inject_cleanup_wait_timeouts(&self, count: usize) {
        self.cleanup_wait_timeouts.store(count, Ordering::Release);
    }

    pub(crate) fn observations(&self) -> TestObservations {
        self.observations
            .lock()
            .expect("test observations should be available")
            .clone()
    }

    fn pause(&self, point: TestPoint, pid: Option<u32>) -> io::Result<()> {
        let Some(pause) = self.pauses.get(&point) else {
            return Ok(());
        };
        pause.reached.send(TestEvent { point, pid }).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "test pause receiver was dropped")
        })?;
        pause
            .release
            .lock()
            .map_err(|_| io::Error::other("test pause lock was poisoned"))?
            .recv_timeout(HOOK_TIMEOUT)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    io::Error::new(io::ErrorKind::TimedOut, "test pause release timed out")
                }
                RecvTimeoutError::Disconnected => io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "test pause release was disconnected",
                ),
            })
    }
}

impl Default for TestHooks {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static ACTIVE_HOOKS: RefCell<Option<Arc<TestHooks>>> = const { RefCell::new(None) };
}

struct HookReset(Option<Arc<TestHooks>>);

impl Drop for HookReset {
    fn drop(&mut self) {
        ACTIVE_HOOKS.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

pub(crate) fn with_hooks<T>(hooks: Arc<TestHooks>, action: impl FnOnce() -> T) -> T {
    let previous = ACTIVE_HOOKS.with(|slot| slot.replace(Some(hooks)));
    let _reset = HookReset(previous);
    action()
}

pub(crate) fn raw_handle_is_valid(handle: usize) -> bool {
    let mut flags = 0;
    unsafe { GetHandleInformation(handle as HANDLE, &mut flags) != 0 }
}

pub(super) fn pause(point: TestPoint, pid: Option<u32>) -> io::Result<()> {
    with_active(|hooks| hooks.pause(point, pid)).unwrap_or(Ok(()))
}

pub(super) fn fail_job_list_update() -> bool {
    with_active(|hooks| hooks.fail_job_list_update).unwrap_or(false)
}

pub(super) fn fail_resume() -> bool {
    with_active(|hooks| hooks.fail_resume).unwrap_or(false)
}

pub(super) fn consume_cleanup_wait_timeout() -> bool {
    with_active(|hooks| {
        hooks
            .cleanup_wait_timeouts
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count > 0).then(|| count - 1)
            })
            .is_ok()
    })
    .unwrap_or(false)
}

pub(super) fn record_attribute_list_initialized(
    count: u32,
    inherited_handles: &[HANDLE],
    job_handles: &[HANDLE],
) {
    let _ = with_active(|hooks| {
        let mut observations = hooks
            .observations
            .lock()
            .expect("test observations should be available");
        observations.attribute_count = Some(count);
        observations.attribute_lists_initialized += 1;
        observations.inherited_handles = inherited_handles
            .iter()
            .map(|handle| *handle as usize)
            .collect();
        observations.job_handles = job_handles.iter().map(|handle| *handle as usize).collect();
        observations.inherited_handle_flags = inherited_handles
            .iter()
            .map(|handle| handle_flags(*handle))
            .collect();
        observations.job_handle_flags = job_handles
            .iter()
            .map(|handle| handle_flags(*handle))
            .collect();
    });
}

pub(super) fn record_handle_list_updated() {
    let _ = with_active(|hooks| {
        hooks
            .observations
            .lock()
            .expect("test observations should be available")
            .handle_list_updates += 1;
    });
}

pub(super) fn record_job_list_updated() {
    let _ = with_active(|hooks| {
        hooks
            .observations
            .lock()
            .expect("test observations should be available")
            .job_list_updates += 1;
    });
}

pub(super) fn record_attribute_list_deleted() {
    let _ = with_active(|hooks| {
        hooks
            .observations
            .lock()
            .expect("test observations should be available")
            .attribute_lists_deleted += 1;
    });
}

pub(super) fn record_create_process_call() {
    let _ = with_active(|hooks| {
        hooks
            .observations
            .lock()
            .expect("test observations should be available")
            .create_process_calls += 1;
    });
}

pub(super) fn record_created_process(pid: u32, in_job: bool) {
    let _ = with_active(|hooks| {
        let mut observations = hooks
            .observations
            .lock()
            .expect("test observations should be available");
        observations.created_pids.push(pid);
        observations.process_in_job.push(in_job);
    });
}

fn with_active<T>(action: impl FnOnce(&TestHooks) -> T) -> Option<T> {
    ACTIVE_HOOKS.with(|slot| slot.borrow().as_deref().map(action))
}

fn handle_flags(handle: HANDLE) -> u32 {
    let mut flags = 0;
    let valid = unsafe { GetHandleInformation(handle, &mut flags) };
    assert_ne!(valid, 0, "test-observed handle should be valid");
    flags
}

pub(crate) fn handle_is_inheritable(flags: u32) -> bool {
    flags & HANDLE_FLAG_INHERIT != 0
}
