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
    detect_registered_project_with_config(project_id, state, ScanConfig::default()).await
}

pub(super) async fn detect_registered_project_with_config(
    project_id: String,
    state: &ProjectsState,
    config: ScanConfig,
) -> Result<DetectionResult, DetectionError> {
    let (project_root, registered_services) = {
        let store = state.store()?;
        let project_root = store.registered_path(&project_id)?;
        let registered_services = store
            .list_services(&project_id)?
            .into_iter()
            .map(|service| (service.working_directory, service.command))
            .collect::<Vec<_>>();
        (project_root, registered_services)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut result = scan_project(&project_id, &project_root, config)?;
        mark_registered_matches(&mut result, &registered_services);
        Ok(result)
    })
    .await
    .map_err(|error| DetectionError::background_task(error.to_string()))?
}

fn mark_registered_matches(result: &mut DetectionResult, registered_services: &[(String, String)]) {
    for suggestion in &mut result.suggestions {
        let matches = registered_services.iter().any(|(directory, command)| {
            working_directories_match(directory, &suggestion.working_directory)
                && command == &suggestion.command
        });
        if matches {
            suggestion.default_selected = false;
            suggestion.warnings.push(super::model::DetectionWarning {
                kind: super::model::DetectionWarningKind::MatchesRegisteredService,
                message: "This suggestion matches an already registered service.".to_owned(),
                path: Some(suggestion.source_path.clone()),
            });
        }
    }
}

fn working_directories_match(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.to_lowercase() == right.to_lowercase()
    }

    #[cfg(not(windows))]
    {
        left == right
    }
}
