use crate::projects::{CommandError, ProjectsState, ServiceStartPreview};
use crate::runtime::manager::RuntimeManager;
use crate::runtime::model::{ServiceLogsSnapshot, ServiceRuntimeSnapshot};
use tauri::State;

#[tauri::command]
pub(crate) fn get_service_start_preview(
    project_id: String,
    service_id: String,
    projects: State<'_, ProjectsState>,
) -> Result<ServiceStartPreview, CommandError> {
    projects
        .store()?
        .prepare_service_start(&project_id, &service_id)
        .map(|launch| launch.preview())
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn start_service(
    project_id: String,
    service_id: String,
    projects: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<ServiceRuntimeSnapshot, CommandError> {
    let (launch, reservation) = {
        let store = projects.store()?;
        let launch = store.prepare_service_start(&project_id, &service_id)?;
        let reservation = runtime.reserve_start(&project_id, &service_id)?;
        (launch, reservation)
    };
    let manager = RuntimeManager::clone(&runtime);

    tauri::async_runtime::spawn_blocking(move || manager.launch_reserved(launch, reservation))
        .await
        .map_err(|error| CommandError::runtime_task(error.to_string()))?
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn stop_service(
    project_id: String,
    service_id: String,
    projects: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<ServiceRuntimeSnapshot, CommandError> {
    projects
        .store()?
        .ensure_service_exists(&project_id, &service_id)?;
    let manager = RuntimeManager::clone(&runtime);

    tauri::async_runtime::spawn_blocking(move || manager.stop(&project_id, &service_id))
        .await
        .map_err(|error| CommandError::runtime_task(error.to_string()))?
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn get_service_runtime(
    project_id: String,
    service_id: String,
    projects: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<ServiceRuntimeSnapshot, CommandError> {
    projects
        .store()?
        .ensure_service_exists(&project_id, &service_id)?;
    runtime
        .get_runtime(&project_id, &service_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn get_service_logs(
    project_id: String,
    service_id: String,
    projects: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<ServiceLogsSnapshot, CommandError> {
    projects
        .store()?
        .ensure_service_exists(&project_id, &service_id)?;
    runtime
        .get_logs(&project_id, &service_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn clear_service_logs(
    project_id: String,
    service_id: String,
    projects: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<ServiceLogsSnapshot, CommandError> {
    projects
        .store()?
        .ensure_service_exists(&project_id, &service_id)?;
    runtime
        .clear_logs(&project_id, &service_id)
        .map_err(CommandError::from)
}
