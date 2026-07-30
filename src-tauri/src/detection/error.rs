use crate::projects::ProjectError;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectionError {
    pub code: String,
    pub message: String,
}

impl DetectionError {
    pub(super) fn background_task(reason: String) -> Self {
        Self {
            code: "detection_task_failed".to_owned(),
            message: format!("The background detection task could not complete. {reason}"),
        }
    }

    pub(super) fn inaccessible_root(path: &Path, reason: String) -> Self {
        Self {
            code: "path_unavailable".to_owned(),
            message: format!(
                "The project folder cannot be scanned ({}): {reason}",
                path.display()
            ),
        }
    }
}

impl From<ProjectError> for DetectionError {
    fn from(error: ProjectError) -> Self {
        let code = match &error {
            ProjectError::InvalidPath { .. } => "invalid_path",
            ProjectError::DirectoryNotFound { .. } => "directory_not_found",
            ProjectError::NotDirectory { .. } => "not_a_directory",
            ProjectError::PathUnavailable { .. } => "path_unavailable",
            ProjectError::ProjectNotFound { .. } => "project_not_found",
            ProjectError::CorruptData { .. } => "data_file_corrupt",
            ProjectError::InvalidData { .. } => "data_file_invalid",
            ProjectError::UnsupportedVersion { .. } => "unsupported_data_version",
            ProjectError::Persistence { .. } => "persistence_failed",
            ProjectError::StateUnavailable => "state_unavailable",
            _ => "detection_failed",
        };

        Self {
            code: code.to_owned(),
            message: error.to_string(),
        }
    }
}
