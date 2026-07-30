use crate::projects::error::{CommandError, ProjectError};
use crate::projects::model::{Project, ServiceDefinition, ServiceInput};
use crate::projects::paths::normalize_existing_directory;
use crate::projects::ProjectsState;
use crate::runtime::RuntimeManager;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub(crate) fn list_projects(state: State<'_, ProjectsState>) -> Result<Vec<Project>, CommandError> {
    state.store()?.list_projects().map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn add_project(
    root_path: String,
    name: Option<String>,
    state: State<'_, ProjectsState>,
) -> Result<Vec<Project>, CommandError> {
    state
        .store()?
        .add_project(&root_path, name.as_deref())
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn rename_project(
    project_id: String,
    name: String,
    state: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Vec<Project>, CommandError> {
    let store = state.store()?;
    runtime.ensure_project_inactive(&project_id, "rename")?;
    store
        .rename_project(&project_id, &name)
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn remove_project(
    project_id: String,
    state: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Vec<Project>, CommandError> {
    let projects = {
        let store = state.store()?;
        runtime.ensure_project_inactive(&project_id, "remove")?;
        store.remove_project(&project_id)?
    };
    runtime.forget_project(&project_id)?;
    Ok(projects)
}

#[tauri::command]
pub(crate) fn open_project_folder(
    project_id: String,
    app: AppHandle,
    state: State<'_, ProjectsState>,
) -> Result<(), CommandError> {
    let registered_path = state.store()?.registered_path(&project_id)?;
    let folder = normalize_existing_directory(registered_path.to_string_lossy().as_ref())?;
    let folder_text = folder.to_string_lossy().into_owned();

    app.opener()
        .open_path(folder_text, None::<&str>)
        .map_err(|error| {
            ProjectError::OpenFolder {
                path: folder,
                reason: error.to_string(),
            }
            .into()
        })
}

#[tauri::command]
pub(crate) fn list_services(
    project_id: String,
    state: State<'_, ProjectsState>,
) -> Result<Vec<ServiceDefinition>, CommandError> {
    state
        .store()?
        .list_services(&project_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn add_service(
    project_id: String,
    service: ServiceInput,
    state: State<'_, ProjectsState>,
) -> Result<Vec<ServiceDefinition>, CommandError> {
    state
        .store()?
        .add_service(&project_id, &service)
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn update_service(
    project_id: String,
    service_id: String,
    service: ServiceInput,
    state: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Vec<ServiceDefinition>, CommandError> {
    let store = state.store()?;
    runtime.ensure_service_inactive(&project_id, &service_id, "edit")?;
    store
        .update_service(&project_id, &service_id, &service)
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn remove_service(
    project_id: String,
    service_id: String,
    state: State<'_, ProjectsState>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Vec<ServiceDefinition>, CommandError> {
    let services = {
        let store = state.store()?;
        runtime.ensure_service_inactive(&project_id, &service_id, "remove")?;
        store.remove_service(&project_id, &service_id)?
    };
    runtime.forget_service(&project_id, &service_id)?;
    Ok(services)
}

#[tauri::command]
pub(crate) fn resolve_service_directory(
    project_id: String,
    selected_path: String,
    state: State<'_, ProjectsState>,
) -> Result<String, CommandError> {
    state
        .store()?
        .resolve_service_directory(&project_id, &selected_path)
        .map_err(CommandError::from)
}
