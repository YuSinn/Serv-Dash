use crate::projects::error::{CommandError, ProjectError};
use crate::projects::model::{
    AddDetectedServicesResult, DetectedServiceSubmission, Project, ServiceDefinition, ServiceInput,
};
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
pub(crate) fn add_detected_services(
    project_id: String,
    submissions: Vec<DetectedServiceSubmission>,
    state: State<'_, ProjectsState>,
) -> Result<AddDetectedServicesResult, CommandError> {
    state
        .store()?
        .add_detected_services(&project_id, &submissions)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::model::{
        AddedDetectedService, DetectedServiceSkipKind, SkippedDetectedService,
    };
    use serde_json::json;

    fn service_definition() -> ServiceDefinition {
        ServiceDefinition {
            id: "00000000-0000-4000-8000-000000000001".to_owned(),
            name: "Detected".to_owned(),
            working_directory: "apps/api".to_owned(),
            command: "npm run -- dev".to_owned(),
            expected_port: Some(3_000),
            local_url: Some("http://localhost:3000".to_owned()),
            created_at: "2026-07-31T00:00:00.000Z".to_owned(),
            updated_at: "2026-07-31T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn detected_submission_deserializes_camel_case_and_preserves_values() {
        let submission: DetectedServiceSubmission = serde_json::from_value(json!({
            "stableId": "  opaque-id  ",
            "service": {
                "name": "Detected",
                "workingDirectory": "apps/api",
                "command": "npm run -- dev",
                "expectedPort": 3000,
                "localUrl": "http://localhost:3000"
            }
        }))
        .expect("camelCase submission should deserialize");

        assert_eq!(submission.stable_id, "  opaque-id  ");
        assert_eq!(submission.service.working_directory, "apps/api");
        assert_eq!(submission.service.expected_port, Some(3_000));
        assert_eq!(
            submission.service.local_url.as_deref(),
            Some("http://localhost:3000")
        );
    }

    #[test]
    fn detected_submission_accepts_null_options_and_rejects_invalid_shapes() {
        let payload = json!({
            "stableId": "stable",
            "service": {
                "name": "Detected",
                "workingDirectory": ".",
                "command": "npm run -- dev",
                "expectedPort": null,
                "localUrl": null
            }
        });
        let submission: DetectedServiceSubmission =
            serde_json::from_value(payload.clone()).expect("null options should deserialize");
        assert_eq!(submission.service.expected_port, None);
        assert_eq!(submission.service.local_url, None);

        let mut unknown = payload.clone();
        unknown["unknownField"] = json!(true);
        assert!(serde_json::from_value::<DetectedServiceSubmission>(unknown).is_err());

        let mut unknown_service_field = payload.clone();
        unknown_service_field["service"]["unknownField"] = json!(true);
        assert!(
            serde_json::from_value::<DetectedServiceSubmission>(unknown_service_field).is_err()
        );

        let mut missing_stable_id = payload.clone();
        missing_stable_id
            .as_object_mut()
            .expect("payload should be an object")
            .remove("stableId");
        assert!(serde_json::from_value::<DetectedServiceSubmission>(missing_stable_id).is_err());

        let mut missing_service = payload;
        missing_service
            .as_object_mut()
            .expect("payload should be an object")
            .remove("service");
        assert!(serde_json::from_value::<DetectedServiceSubmission>(missing_service).is_err());
    }

    #[test]
    fn detected_batch_response_serializes_the_camel_case_contract() {
        let service = service_definition();
        let result = AddDetectedServicesResult {
            added: vec![AddedDetectedService {
                stable_id: "added-stable".to_owned(),
                service: service.clone(),
            }],
            skipped: vec![SkippedDetectedService {
                stable_id: "skipped-stable".to_owned(),
                name: "Duplicate".to_owned(),
                kind: DetectedServiceSkipKind::DuplicateBatchName,
                message: "Another selected service uses this name.".to_owned(),
            }],
            services: vec![service],
        };

        let value = serde_json::to_value(result).expect("batch response should serialize");
        assert_eq!(value["added"][0]["stableId"], "added-stable");
        assert_eq!(value["added"][0]["service"]["workingDirectory"], "apps/api");
        assert_eq!(value["added"][0]["service"]["expectedPort"], 3_000);
        assert_eq!(
            value["added"][0]["service"]["localUrl"],
            "http://localhost:3000"
        );
        assert!(value["added"][0]["service"].get("stableId").is_none());
        assert_eq!(value["skipped"][0]["stableId"], "skipped-stable");
        assert_eq!(value["skipped"][0]["kind"], "duplicateBatchName");
        assert!(value["skipped"][0].get("service").is_none());
        assert_eq!(
            value["services"][0]["id"],
            value["added"][0]["service"]["id"]
        );
    }

    #[test]
    fn global_project_error_remains_a_structured_command_error() {
        let error = CommandError::from(ProjectError::ProjectNotFound {
            project_id: "missing-project".to_owned(),
        });

        let value = serde_json::to_value(error).expect("command error should serialize");
        assert_eq!(value["code"], "project_not_found");
        assert!(value["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing-project")));
    }
}
