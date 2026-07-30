mod commands;
mod emitter;
mod io_tasks;
mod manager;
mod model;
mod state;

#[cfg(windows)]
mod windows_process;

#[cfg(not(windows))]
compile_error!("Server Dashboard service execution is currently supported only on Windows.");

pub(crate) use commands::{
    clear_service_logs, get_service_logs, get_service_runtime, get_service_start_preview,
    start_service, stop_service,
};
pub(crate) use manager::RuntimeManager;

#[cfg(all(test, windows))]
#[path = "runtime_test_support.rs"]
mod test_support;

#[cfg(all(test, windows))]
#[path = "runtime_validation_tests.rs"]
mod validation_tests;

#[cfg(all(test, windows))]
#[path = "runtime_process_tests.rs"]
mod process_tests;

#[cfg(all(test, windows))]
#[path = "runtime_revision_tests.rs"]
mod revision_tests;

#[cfg(all(test, windows))]
#[path = "runtime_windows_job_tests.rs"]
mod windows_job_tests;
