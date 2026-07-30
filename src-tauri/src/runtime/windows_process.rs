use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

#[cfg(test)]
#[path = "windows_process_test_api.rs"]
pub(crate) mod test_api;

pub(crate) const FORCED_STOP_EXIT_CODE: u32 = 0xE000_0001;
const START_FAILURE_EXIT_CODE: u32 = 0xE000_0002;
const START_CLEANUP_WAIT_MS: u32 = 2_000;
const PROCESS_ATTRIBUTE_COUNT: u32 = 2;

pub(crate) struct RunControl {
    job: Mutex<Option<OwnedHandle>>,
    process: OwnedHandle,
}

impl RunControl {
    fn new(job: OwnedHandle, process: OwnedHandle) -> Self {
        Self {
            job: Mutex::new(Some(job)),
            process,
        }
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        let job = self
            .job
            .lock()
            .map_err(|_| io::Error::other("the process Job handle is unavailable"))?;
        let Some(job) = job.as_ref() else {
            return Ok(());
        };

        let result = unsafe { TerminateJobObject(raw_handle(job), FORCED_STOP_EXIT_CODE) };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(crate) fn close_job(&self) -> io::Result<()> {
        let mut slot = self
            .job
            .lock()
            .map_err(|_| io::Error::other("the process Job handle is unavailable"))?;
        let Some(job) = slot.take() else {
            return Ok(());
        };

        match close_owned_handle(job) {
            Ok(()) => Ok(()),
            Err((error, job)) => {
                *slot = Some(job);
                Err(error)
            }
        }
    }

    pub(crate) fn wait(&self, timeout_ms: u32) -> io::Result<bool> {
        wait_for_handle(raw_handle(&self.process), timeout_ms)
    }

    pub(crate) fn exit_code(&self) -> io::Result<u32> {
        exit_code(raw_handle(&self.process))
    }

    pub(crate) fn terminate_close_and_wait(&self, timeout_ms: u32) -> io::Result<()> {
        let mut failures = Vec::new();
        let mut close_failed = false;

        if let Err(error) = self.terminate() {
            failures.push(format!("TerminateJobObject failed: {error}"));
        }
        if let Err(error) = self.close_job() {
            close_failed = true;
            failures.push(format!("closing the Job handle failed: {error}"));
        }

        let mut terminated = match wait_for_cleanup_handle(raw_handle(&self.process), timeout_ms) {
            Ok(true) => true,
            Ok(false) => {
                failures.push(format!(
                    "waiting for the process timed out after {timeout_ms} ms"
                ));
                false
            }
            Err(error) => {
                failures.push(format!("waiting for the process failed: {error}"));
                false
            }
        };

        if !terminated {
            match wait_for_handle(raw_handle(&self.process), 0) {
                Ok(true) => terminated = true,
                Ok(false) => {
                    let result = unsafe {
                        TerminateProcess(raw_handle(&self.process), START_FAILURE_EXIT_CODE)
                    };
                    if result == 0 {
                        failures.push(format!(
                            "TerminateProcess fallback failed: {}",
                            io::Error::last_os_error()
                        ));
                    }

                    match wait_for_handle(raw_handle(&self.process), timeout_ms) {
                        Ok(true) => terminated = true,
                        Ok(false) => failures.push(format!(
                            "the process was not signaled after the {timeout_ms} ms fallback wait"
                        )),
                        Err(error) => {
                            failures.push(format!("the fallback process wait failed: {error}"))
                        }
                    }
                }
                Err(error) => failures.push(format!(
                    "checking the process after the cleanup wait failed: {error}"
                )),
            }
        }

        if close_failed {
            if let Err(error) = self.close_job() {
                failures.push(format!("retrying the Job handle close failed: {error}"));
            }
        }
        if !terminated {
            failures.push("process termination could not be confirmed".to_owned());
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(failures.join("; ")))
        }
    }
}

impl Drop for RunControl {
    fn drop(&mut self) {
        if let Err(error) = self.close_job() {
            eprintln!("Server Dashboard could not close a service Job handle: {error}");
        }
    }
}

pub(crate) struct SpawnedProcess {
    pub(crate) pid: u32,
    control: Arc<RunControl>,
    wait_process: Option<OwnedHandle>,
    primary_thread: Option<OwnedHandle>,
    stdout: Option<File>,
    stderr: Option<File>,
    resolved: bool,
}

impl SpawnedProcess {
    pub(crate) fn control(&self) -> Arc<RunControl> {
        Arc::clone(&self.control)
    }

    pub(crate) fn take_wait_process(&mut self) -> OwnedHandle {
        self.wait_process
            .take()
            .expect("wait process handle can only be taken once")
    }

    pub(crate) fn take_stdout(&mut self) -> File {
        self.stdout
            .take()
            .expect("stdout handle can only be taken once")
    }

    pub(crate) fn take_stderr(&mut self) -> File {
        self.stderr
            .take()
            .expect("stderr handle can only be taken once")
    }

    pub(crate) fn resume(&mut self) -> io::Result<()> {
        #[cfg(test)]
        if test_api::fail_resume() {
            return Err(io::Error::other("injected ResumeThread failure"));
        }

        let thread = self
            .primary_thread
            .as_ref()
            .expect("the primary thread must exist before resume");
        let previous_count = unsafe { ResumeThread(raw_handle(thread)) };
        if previous_count == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        if previous_count != 1 {
            return Err(io::Error::other(format!(
                "ResumeThread returned unexpected suspend count {previous_count}"
            )));
        }

        self.primary_thread.take();
        self.resolved = true;
        Ok(())
    }

    pub(crate) fn terminate_before_running(&mut self) -> io::Result<()> {
        self.primary_thread.take();
        let result = self.control.terminate_close_and_wait(START_CLEANUP_WAIT_MS);
        match self.control.wait(0) {
            Ok(true) => self.resolved = true,
            Ok(false) => {
                return Err(append_error(
                    result.err(),
                    "process termination was not confirmed after start cleanup",
                ));
            }
            Err(error) => {
                return Err(append_error(
                    result.err(),
                    &format!("the final process cleanup check failed: {error}"),
                ));
            }
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn pause_before_resume(&self) -> io::Result<()> {
        test_api::pause(test_api::TestPoint::BeforeResume, Some(self.pid))
    }
}

impl Drop for SpawnedProcess {
    fn drop(&mut self) {
        if !self.resolved {
            if let Err(error) = self.terminate_before_running() {
                eprintln!("Server Dashboard could not clean up a suspended process: {error}");
            }
        }
    }
}

pub(crate) fn spawn_command(
    command: &str,
    working_directory: &Path,
    should_cancel: impl Fn() -> bool,
) -> io::Result<SpawnedProcess> {
    let job = create_kill_on_close_job()?;
    let (stdout_read, stdout_write) = output_pipe()?;
    let (stderr_read, stderr_write) = output_pipe()?;
    let (stdin_read, stdin_write) = input_pipe()?;

    let inherited_handles = [
        raw_handle(&stdin_read),
        raw_handle(&stdout_write),
        raw_handle(&stderr_write),
    ];
    let job_handles = [raw_handle(&job)];
    let attributes = ProcessThreadAttributes::new(&inherited_handles, &job_handles)?;

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = inherited_handles[0];
    startup.StartupInfo.hStdOutput = inherited_handles[1];
    startup.StartupInfo.hStdError = inherited_handles[2];
    startup.lpAttributeList = attributes.pointer();

    let cmd_path = system_cmd_path()?;
    let application = wide_null(cmd_path.as_os_str());
    let mut command_line = OsStr::new(&format!("cmd.exe /D /S /C \"{command}\""))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let current_directory = wide_null(working_directory.as_os_str());
    let mut process_info = PROCESS_INFORMATION::default();

    #[cfg(test)]
    test_api::pause(test_api::TestPoint::BeforeCreateProcess, None)?;
    if should_cancel() {
        return Err(shutdown_start_error());
    }
    #[cfg(test)]
    test_api::pause(test_api::TestPoint::CreateProcessEntered, None)?;
    #[cfg(test)]
    test_api::record_create_process_call();

    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            current_directory.as_ptr(),
            &startup.StartupInfo,
            &mut process_info,
        )
    };
    drop(attributes);
    if created == 0 {
        return Err(io::Error::last_os_error());
    }

    // CreateProcessW guarantees both handles are valid when it succeeds.
    let process = unsafe { OwnedHandle::from_raw_handle(process_info.hProcess) };
    let primary_thread = unsafe { OwnedHandle::from_raw_handle(process_info.hThread) };
    drop(stdin_read);
    drop(stdin_write);
    drop(stdout_write);
    drop(stderr_write);

    let in_job = match process_is_in_job(&process, &job) {
        Ok(in_job) => in_job,
        Err(error) => {
            return Err(fail_created_process(
                contextual_error("IsProcessInJob failed after CreateProcessW", error),
                job,
                process,
                primary_thread,
            ));
        }
    };
    #[cfg(test)]
    test_api::record_created_process(process_info.dwProcessId, in_job);
    if !in_job {
        return Err(fail_created_process(
            io::Error::other(
                "CreateProcessW returned a process that was not associated through PROC_THREAD_ATTRIBUTE_JOB_LIST",
            ),
            job,
            process,
            primary_thread,
        ));
    }

    #[cfg(test)]
    if let Err(error) = test_api::pause(
        test_api::TestPoint::AfterCreateProcess,
        Some(process_info.dwProcessId),
    ) {
        return Err(fail_created_process(error, job, process, primary_thread));
    }
    if should_cancel() {
        return Err(fail_created_process(
            shutdown_start_error(),
            job,
            process,
            primary_thread,
        ));
    }

    let control_process = match duplicate_handle(&process) {
        Ok(process) => process,
        Err(error) => {
            return Err(fail_created_process(
                contextual_error("DuplicateHandle failed for the process control", error),
                job,
                process,
                primary_thread,
            ));
        }
    };
    let control = Arc::new(RunControl::new(job, control_process));

    Ok(SpawnedProcess {
        pid: process_info.dwProcessId,
        control,
        wait_process: Some(process),
        primary_thread: Some(primary_thread),
        stdout: Some(handle_to_file(stdout_read)),
        stderr: Some(handle_to_file(stderr_read)),
        resolved: false,
    })
}

pub(crate) fn wait_process(handle: &OwnedHandle) -> io::Result<u32> {
    if !wait_for_handle(raw_handle(handle), INFINITE)? {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the process wait unexpectedly timed out",
        ));
    }
    exit_code(raw_handle(handle))
}

fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    let raw_job = unsafe { CreateJobObjectW(null(), null()) };
    let job = unsafe { owned_handle(raw_job)? };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    let configured = unsafe {
        SetInformationJobObject(
            raw_handle(&job),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(job)
    }
}

fn process_is_in_job(process: &OwnedHandle, job: &OwnedHandle) -> io::Result<bool> {
    let mut in_job = 0;
    let result = unsafe { IsProcessInJob(raw_handle(process), raw_handle(job), &mut in_job) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(in_job != 0)
    }
}

fn fail_created_process(
    operation_error: io::Error,
    job: OwnedHandle,
    process: OwnedHandle,
    primary_thread: OwnedHandle,
) -> io::Error {
    drop(primary_thread);
    let control = RunControl::new(job, process);
    match control.terminate_close_and_wait(START_CLEANUP_WAIT_MS) {
        Ok(()) => operation_error,
        Err(cleanup_error) => io::Error::other(format!(
            "{operation_error}; cleanup also failed: {cleanup_error}"
        )),
    }
}

fn output_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    create_pipe_with_parent_end(false)
}

fn input_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (read, write) = create_pipe()?;
    make_non_inheritable(&write)?;
    Ok((read, write))
}

fn create_pipe_with_parent_end(parent_is_write: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (read, write) = create_pipe()?;
    if parent_is_write {
        make_non_inheritable(&write)?;
    } else {
        make_non_inheritable(&read)?;
    }
    Ok((read, write))
}

fn create_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read = null_mut();
    let mut write = null_mut();
    let created = unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(unsafe { (owned_handle(read)?, owned_handle(write)?) })
}

fn make_non_inheritable(handle: &OwnedHandle) -> io::Result<()> {
    let updated = unsafe { SetHandleInformation(raw_handle(handle), HANDLE_FLAG_INHERIT, 0) };
    if updated == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn duplicate_handle(handle: &OwnedHandle) -> io::Result<OwnedHandle> {
    let current_process = unsafe { GetCurrentProcess() };
    let mut duplicate = null_mut();
    let duplicated = unsafe {
        DuplicateHandle(
            current_process,
            raw_handle(handle),
            current_process,
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if duplicated == 0 {
        Err(io::Error::last_os_error())
    } else {
        unsafe { owned_handle(duplicate) }
    }
}

fn system_cmd_path() -> io::Result<PathBuf> {
    let mut buffer = vec![0_u16; 260];
    loop {
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if (length as usize) < buffer.len() {
            let mut path = PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
            path.push("cmd.exe");
            return Ok(path);
        }
        buffer.resize(length as usize + 1, 0);
    }
}

fn wait_for_cleanup_handle(handle: HANDLE, timeout_ms: u32) -> io::Result<bool> {
    #[cfg(test)]
    if test_api::consume_cleanup_wait_timeout() {
        return Ok(false);
    }
    wait_for_handle(handle, timeout_ms)
}

fn wait_for_handle(handle: HANDLE, timeout_ms: u32) -> io::Result<bool> {
    match unsafe { WaitForSingleObject(handle, timeout_ms) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        result => Err(io::Error::other(format!(
            "unexpected Windows wait result {result}"
        ))),
    }
}

fn exit_code(handle: HANDLE) -> io::Result<u32> {
    let mut code = 0;
    let result = unsafe { GetExitCodeProcess(handle, &mut code) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(code)
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}

unsafe fn owned_handle(raw: HANDLE) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

fn close_owned_handle(handle: OwnedHandle) -> Result<(), (io::Error, OwnedHandle)> {
    let raw = handle.into_raw_handle();
    if unsafe { CloseHandle(raw) } == 0 {
        let error = io::Error::last_os_error();
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        Err((error, handle))
    } else {
        Ok(())
    }
}

fn handle_to_file(handle: OwnedHandle) -> File {
    unsafe { File::from_raw_handle(handle.into_raw_handle()) }
}

fn shutdown_start_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "Server Dashboard shutdown began before process creation completed",
    )
}

fn contextual_error(context: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{context}: {error}"))
}

fn append_error(existing: Option<io::Error>, addition: &str) -> io::Error {
    match existing {
        Some(error) => io::Error::other(format!("{error}; {addition}")),
        None => io::Error::other(addition.to_owned()),
    }
}

struct ProcessThreadAttributes<'a> {
    storage: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
    _inherited_handles: &'a [HANDLE],
    _job_handles: &'a [HANDLE],
}

impl<'a> ProcessThreadAttributes<'a> {
    fn new(inherited_handles: &'a [HANDLE], job_handles: &'a [HANDLE]) -> io::Result<Self> {
        let mut required_bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(
                null_mut(),
                PROCESS_ATTRIBUTE_COUNT,
                0,
                &mut required_bytes,
            );
        }
        if required_bytes == 0 {
            return Err(io::Error::last_os_error());
        }

        let words = required_bytes.div_ceil(size_of::<usize>());
        let mut storage = vec![0_usize; words];
        let pointer = storage.as_mut_ptr().cast();
        let initialized = unsafe {
            InitializeProcThreadAttributeList(
                pointer,
                PROCESS_ATTRIBUTE_COUNT,
                0,
                &mut required_bytes,
            )
        };
        if initialized == 0 {
            return Err(io::Error::last_os_error());
        }

        #[cfg(test)]
        test_api::record_attribute_list_initialized(
            PROCESS_ATTRIBUTE_COUNT,
            inherited_handles,
            job_handles,
        );

        let attributes = Self {
            storage,
            pointer,
            _inherited_handles: inherited_handles,
            _job_handles: job_handles,
        };
        attributes.update_handle_list(inherited_handles)?;
        attributes.update_job_list(job_handles)?;
        Ok(attributes)
    }

    fn update_handle_list(&self, inherited_handles: &[HANDLE]) -> io::Result<()> {
        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited_handles.as_ptr().cast(),
                size_of_val(inherited_handles),
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            Err(contextual_error(
                "PROC_THREAD_ATTRIBUTE_HANDLE_LIST could not be configured",
                io::Error::last_os_error(),
            ))
        } else {
            #[cfg(test)]
            test_api::record_handle_list_updated();
            Ok(())
        }
    }

    fn update_job_list(&self, job_handles: &[HANDLE]) -> io::Result<()> {
        #[cfg(test)]
        if test_api::fail_job_list_update() {
            return Err(job_list_error(io::Error::from_raw_os_error(50)));
        }

        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                job_handles.as_ptr().cast(),
                size_of_val(job_handles),
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            Err(job_list_error(io::Error::last_os_error()))
        } else {
            #[cfg(test)]
            test_api::record_job_list_updated();
            Ok(())
        }
    }

    fn pointer(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.pointer
    }
}

impl Drop for ProcessThreadAttributes<'_> {
    fn drop(&mut self) {
        let _keep_storage_alive = &self.storage;
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
        #[cfg(test)]
        test_api::record_attribute_list_deleted();
    }
}

fn job_list_error(error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "PROC_THREAD_ATTRIBUTE_JOB_LIST could not be configured. Server Dashboard process execution requires Windows 10 or later: {error}"
        ),
    )
}
