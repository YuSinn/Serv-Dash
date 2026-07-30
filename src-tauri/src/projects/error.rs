use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum ProjectError {
    InvalidPath {
        reason: String,
    },
    DirectoryNotFound {
        path: PathBuf,
    },
    NotDirectory {
        path: PathBuf,
    },
    PathUnavailable {
        path: PathBuf,
        reason: String,
    },
    DuplicatePath {
        path: PathBuf,
    },
    InvalidName {
        reason: String,
    },
    ProjectNotFound {
        project_id: String,
    },
    ServiceNotFound {
        project_id: String,
        service_id: String,
    },
    DuplicateServiceName {
        name: String,
    },
    InvalidServiceName {
        reason: String,
    },
    InvalidWorkingDirectory {
        reason: String,
    },
    WorkingDirectoryNotFound {
        path: PathBuf,
    },
    WorkingDirectoryNotDirectory {
        path: PathBuf,
    },
    WorkingDirectoryOutsideProject {
        path: PathBuf,
    },
    WorkingDirectoryEscapesProject {
        path: PathBuf,
    },
    InvalidCommand {
        reason: String,
    },
    InvalidPort {
        port: u32,
    },
    InvalidUrl {
        reason: String,
    },
    CorruptData {
        path: PathBuf,
        reason: String,
    },
    InvalidData {
        path: PathBuf,
        reason: String,
    },
    UnsupportedVersion {
        path: PathBuf,
        expected: u32,
        found: u64,
    },
    Persistence {
        action: &'static str,
        path: PathBuf,
        reason: String,
    },
    OpenFolder {
        path: PathBuf,
        reason: String,
    },
    ServiceAlreadyActive {
        project_id: String,
        service_id: String,
        status: String,
    },
    ServiceRuntimeActive {
        project_id: String,
        service_id: String,
        action: &'static str,
        status: String,
    },
    ProjectRuntimeActive {
        project_id: String,
        service_id: String,
        action: &'static str,
        status: String,
    },
    ProcessStart {
        reason: String,
    },
    ProcessStop {
        reason: String,
    },
    RuntimeUnavailable {
        reason: String,
    },
    RuntimeShuttingDown,
    StateUnavailable,
}

impl ProjectError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath { .. } => "invalid_path",
            Self::DirectoryNotFound { .. } => "directory_not_found",
            Self::NotDirectory { .. } => "not_a_directory",
            Self::PathUnavailable { .. } => "path_unavailable",
            Self::DuplicatePath { .. } => "duplicate_project",
            Self::InvalidName { .. } => "invalid_name",
            Self::ProjectNotFound { .. } => "project_not_found",
            Self::ServiceNotFound { .. } => "service_not_found",
            Self::DuplicateServiceName { .. } => "duplicate_service_name",
            Self::InvalidServiceName { .. } => "invalid_service_name",
            Self::InvalidWorkingDirectory { .. } => "invalid_working_directory",
            Self::WorkingDirectoryNotFound { .. } => "working_directory_not_found",
            Self::WorkingDirectoryNotDirectory { .. } => "working_directory_not_directory",
            Self::WorkingDirectoryOutsideProject { .. } => "working_directory_outside_project",
            Self::WorkingDirectoryEscapesProject { .. } => "working_directory_link_escape",
            Self::InvalidCommand { .. } => "invalid_command",
            Self::InvalidPort { .. } => "invalid_port",
            Self::InvalidUrl { .. } => "invalid_url",
            Self::CorruptData { .. } => "data_file_corrupt",
            Self::InvalidData { .. } => "data_file_invalid",
            Self::UnsupportedVersion { .. } => "unsupported_data_version",
            Self::Persistence { .. } => "persistence_failed",
            Self::OpenFolder { .. } => "open_folder_failed",
            Self::ServiceAlreadyActive { .. } => "service_already_active",
            Self::ServiceRuntimeActive { .. } => "service_runtime_active",
            Self::ProjectRuntimeActive { .. } => "project_runtime_active",
            Self::ProcessStart { .. } => "process_start_failed",
            Self::ProcessStop { .. } => "process_stop_failed",
            Self::RuntimeUnavailable { .. } => "runtime_unavailable",
            Self::RuntimeShuttingDown => "runtime_shutting_down",
            Self::StateUnavailable => "state_unavailable",
        }
    }
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { reason } => {
                write!(formatter, "Select a valid absolute project folder. {reason}")
            }
            Self::DirectoryNotFound { path } => write!(
                formatter,
                "The project folder no longer exists: {}",
                path.display()
            ),
            Self::NotDirectory { path } => write!(
                formatter,
                "The selected path is not a folder: {}",
                path.display()
            ),
            Self::PathUnavailable { path, reason } => write!(
                formatter,
                "The project folder cannot be accessed ({}): {reason}",
                path.display()
            ),
            Self::DuplicatePath { path } => write!(
                formatter,
                "This folder is already registered: {}",
                path.display()
            ),
            Self::InvalidName { reason } => {
                write!(formatter, "Enter a valid project name. {reason}")
            }
            Self::ProjectNotFound { project_id } => {
                write!(formatter, "The project is no longer registered ({project_id}).")
            }
            Self::ServiceNotFound {
                project_id,
                service_id,
            } => write!(
                formatter,
                "The service is no longer configured for this project (project {project_id}, service {service_id})."
            ),
            Self::DuplicateServiceName { name } => write!(
                formatter,
                "A service named '{name}' is already configured for this project."
            ),
            Self::InvalidServiceName { reason } => {
                write!(formatter, "Enter a valid service name. {reason}")
            }
            Self::InvalidWorkingDirectory { reason } => {
                write!(formatter, "Enter a valid relative working directory. {reason}")
            }
            Self::WorkingDirectoryNotFound { path } => write!(
                formatter,
                "The working directory does not exist: {}",
                path.display()
            ),
            Self::WorkingDirectoryNotDirectory { path } => write!(
                formatter,
                "The working directory path is not a folder: {}",
                path.display()
            ),
            Self::WorkingDirectoryOutsideProject { path } => write!(
                formatter,
                "The selected working directory is outside the project: {}",
                path.display()
            ),
            Self::WorkingDirectoryEscapesProject { path } => write!(
                formatter,
                "The working directory resolves outside the project through a symbolic link or junction: {}",
                path.display()
            ),
            Self::InvalidCommand { reason } => {
                write!(formatter, "Enter a valid command. {reason}")
            }
            Self::InvalidPort { port } => write!(
                formatter,
                "Expected port {port} is outside the valid range of 1 to 65535."
            ),
            Self::InvalidUrl { reason } => write!(
                formatter,
                "Enter a valid HTTP or HTTPS local URL. {reason}"
            ),
            Self::CorruptData { path, reason } => write!(
                formatter,
                "The project data file is corrupt and was not changed ({}): {reason}",
                path.display()
            ),
            Self::InvalidData { path, reason } => write!(
                formatter,
                "The project data file contains invalid data and was not changed ({}): {reason}",
                path.display()
            ),
            Self::UnsupportedVersion {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "The project data file uses unsupported format version {found}; this app supports version 1 migration and current version {expected} ({}).",
                path.display()
            ),
            Self::Persistence {
                action,
                path,
                reason,
            } => write!(
                formatter,
                "Project data could not be {action} ({}): {reason}",
                path.display()
            ),
            Self::OpenFolder { path, reason } => write!(
                formatter,
                "Windows could not open the project folder ({}): {reason}",
                path.display()
            ),
            Self::ServiceAlreadyActive {
                project_id,
                service_id,
                status,
            } => write!(
                formatter,
                "This service is already {status} (project {project_id}, service {service_id}). Stop it before starting it again."
            ),
            Self::ServiceRuntimeActive {
                project_id,
                service_id,
                action,
                status,
            } => write!(
                formatter,
                "Stop this service before you {action} it. It is currently {status} (project {project_id}, service {service_id})."
            ),
            Self::ProjectRuntimeActive {
                project_id,
                service_id,
                action,
                status,
            } => write!(
                formatter,
                "Stop every active service before you {action} this project. Service {service_id} is currently {status} (project {project_id})."
            ),
            Self::ProcessStart { reason } => {
                write!(formatter, "The configured service could not be started. {reason}")
            }
            Self::ProcessStop { reason } => {
                write!(formatter, "The service process tree could not be stopped. {reason}")
            }
            Self::RuntimeUnavailable { reason } => write!(
                formatter,
                "Service runtime state is temporarily unavailable. {reason}"
            ),
            Self::RuntimeShuttingDown => write!(
                formatter,
                "Server Dashboard is closing and cannot start another service."
            ),
            Self::StateUnavailable => write!(
                formatter,
                "Project storage is temporarily unavailable. Restart Server Dashboard and try again."
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: String,
    message: String,
}

impl From<ProjectError> for CommandError {
    fn from(error: ProjectError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

impl CommandError {
    pub(crate) fn runtime_task(reason: String) -> Self {
        Self {
            code: "runtime_task_failed".to_owned(),
            message: format!("The background runtime task could not complete. {reason}"),
        }
    }
}
