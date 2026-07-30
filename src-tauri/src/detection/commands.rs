use super::error::DetectionError;
use super::model::DetectionResult;
use super::scanner::{scan_project, ScanConfig};
use crate::projects::ProjectsState;
use tauri::State;

#[tauri::command]
pub(crate) async fn detect_project_services(
    project_id: String,
    state: State<'_, ProjectsState>,
) -> Result<DetectionResult, DetectionError> {
    detect_registered_project(project_id, &state).await
}

pub(super) async fn detect_registered_project(
    project_id: String,
    state: &ProjectsState,
) -> Result<DetectionResult, DetectionError> {
    let project_root = state.store()?.registered_path(&project_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_project(&project_id, &project_root, ScanConfig::default())
    })
    .await
    .map_err(|error| DetectionError::background_task(error.to_string()))?
}
