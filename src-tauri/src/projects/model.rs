use crate::projects::error::ProjectError;
use crate::projects::paths::{normalize_relative_directory_syntax, path_key};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use url::Url;
use uuid::Uuid;

pub(crate) const DATA_VERSION: u32 = 2;
pub(crate) const LEGACY_DATA_VERSION: u32 = 1;
const MAX_SERVICE_NAME_LENGTH: usize = 120;
const MAX_COMMAND_LENGTH: usize = 4096;
const MAX_URL_LENGTH: usize = 2048;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub services: Vec<ServiceDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServiceDefinition {
    pub id: String,
    pub name: String,
    pub working_directory: String,
    pub command: String,
    pub expected_port: Option<u16>,
    pub local_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServiceInput {
    pub name: String,
    pub working_directory: String,
    pub command: String,
    pub expected_port: Option<u32>,
    pub local_url: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ServiceLaunchSpec {
    pub project_id: String,
    pub project_name: String,
    pub service_id: String,
    pub service_name: String,
    pub working_directory: PathBuf,
    pub command: String,
}

impl ServiceLaunchSpec {
    pub(crate) fn preview(&self) -> ServiceStartPreview {
        ServiceStartPreview {
            project_id: self.project_id.clone(),
            project_name: self.project_name.clone(),
            service_id: self.service_id.clone(),
            service_name: self.service_name.clone(),
            resolved_working_directory: self.working_directory.to_string_lossy().into_owned(),
            command: self.command.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceStartPreview {
    pub project_id: String,
    pub project_name: String,
    pub service_id: String,
    pub service_name: String,
    pub resolved_working_directory: String,
    pub command: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedData {
    pub version: u32,
    pub projects: Vec<Project>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LegacyPersistedData {
    pub version: u32,
    pub projects: Vec<LegacyProject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LegacyProject {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: String,
    pub updated_at: String,
}

impl PersistedData {
    pub fn empty() -> Self {
        Self {
            version: DATA_VERSION,
            projects: Vec::new(),
        }
    }

    pub fn migrate_from_v1(legacy: LegacyPersistedData) -> Result<Self, String> {
        if legacy.version != LEGACY_DATA_VERSION {
            return Err(format!(
                "legacy format version must be {LEGACY_DATA_VERSION}, found {}",
                legacy.version
            ));
        }

        let migrated = Self {
            version: DATA_VERSION,
            projects: legacy
                .projects
                .into_iter()
                .map(|project| Project {
                    id: project.id,
                    name: project.name,
                    root_path: project.root_path,
                    created_at: project.created_at,
                    updated_at: project.updated_at,
                    services: Vec::new(),
                })
                .collect(),
        };
        migrated.validate()?;
        Ok(migrated)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != DATA_VERSION {
            return Err(format!(
                "format version must be {DATA_VERSION}, found {}",
                self.version
            ));
        }

        let mut project_ids = HashSet::new();
        let mut project_paths = HashSet::new();
        let mut service_ids = HashSet::new();

        for project in &self.projects {
            validate_uuid(&project.id, "project")?;
            if !project_ids.insert(project.id.as_str()) {
                return Err(format!("project id '{}' is duplicated", project.id));
            }

            validate_project_name(project)?;
            if project.root_path.trim().is_empty() || !Path::new(&project.root_path).is_absolute() {
                return Err(format!(
                    "project '{}' does not have a valid absolute root path",
                    project.id
                ));
            }
            if !project_paths.insert(path_key(Path::new(&project.root_path))) {
                return Err(format!(
                    "project root path '{}' is duplicated",
                    project.root_path
                ));
            }
            validate_dates(
                "project",
                &project.id,
                &project.created_at,
                &project.updated_at,
            )?;

            let mut service_names = HashSet::new();
            for service in &project.services {
                validate_uuid(&service.id, "service")?;
                if !service_ids.insert(service.id.as_str()) {
                    return Err(format!("service id '{}' is duplicated", service.id));
                }

                let normalized_name = normalize_service_name(&service.name)
                    .map_err(|error| format!("service '{}': {error}", service.id))?;
                if normalized_name != service.name {
                    return Err(format!(
                        "service '{}' does not have a normalized name",
                        service.id
                    ));
                }
                if !service_names.insert(service_name_key(&service.name)) {
                    return Err(format!(
                        "project '{}' contains duplicate service name '{}'",
                        project.id, service.name
                    ));
                }

                let normalized_directory =
                    normalize_relative_directory_syntax(&service.working_directory)
                        .map_err(|error| format!("service '{}': {error}", service.id))?;
                if normalized_directory != service.working_directory {
                    return Err(format!(
                        "service '{}' does not have a normalized working directory",
                        service.id
                    ));
                }

                let normalized_command = normalize_command(&service.command)
                    .map_err(|error| format!("service '{}': {error}", service.id))?;
                if normalized_command != service.command {
                    return Err(format!(
                        "service '{}' does not have a normalized command",
                        service.id
                    ));
                }

                let normalized_port = normalize_expected_port(service.expected_port.map(u32::from))
                    .map_err(|error| format!("service '{}': {error}", service.id))?;
                if normalized_port != service.expected_port {
                    return Err(format!("service '{}' has an invalid port", service.id));
                }

                let normalized_url = normalize_local_url(service.local_url.as_deref())
                    .map_err(|error| format!("service '{}': {error}", service.id))?;
                if normalized_url != service.local_url {
                    return Err(format!(
                        "service '{}' does not have a normalized local URL",
                        service.id
                    ));
                }

                validate_dates(
                    "service",
                    &service.id,
                    &service.created_at,
                    &service.updated_at,
                )?;
            }
        }

        Ok(())
    }
}

pub(crate) fn normalize_service_name(name: &str) -> Result<String, ProjectError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidServiceName {
            reason: "The service name cannot be empty.".to_owned(),
        });
    }
    if trimmed.chars().count() > MAX_SERVICE_NAME_LENGTH {
        return Err(ProjectError::InvalidServiceName {
            reason: format!("Use at most {MAX_SERVICE_NAME_LENGTH} characters."),
        });
    }
    if trimmed.contains('\0') || trimmed.contains(['\r', '\n']) {
        return Err(ProjectError::InvalidServiceName {
            reason: "NUL characters and line breaks are not allowed.".to_owned(),
        });
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn service_name_key(name: &str) -> String {
    name.to_lowercase()
}

pub(crate) fn normalize_command(command: &str) -> Result<String, ProjectError> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidCommand {
            reason: "The command cannot be empty.".to_owned(),
        });
    }
    if trimmed.chars().count() > MAX_COMMAND_LENGTH {
        return Err(ProjectError::InvalidCommand {
            reason: format!("Use at most {MAX_COMMAND_LENGTH} characters."),
        });
    }
    if trimmed.contains('\0') || trimmed.contains(['\r', '\n']) {
        return Err(ProjectError::InvalidCommand {
            reason: "NUL characters and line breaks are not allowed.".to_owned(),
        });
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn normalize_expected_port(port: Option<u32>) -> Result<Option<u16>, ProjectError> {
    match port {
        None => Ok(None),
        Some(port @ 1..=65_535) => Ok(Some(port as u16)),
        Some(port) => Err(ProjectError::InvalidPort { port }),
    }
}

pub(crate) fn normalize_local_url(url: Option<&str>) -> Result<Option<String>, ProjectError> {
    let Some(url) = url else {
        return Ok(None);
    };
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_URL_LENGTH {
        return Err(ProjectError::InvalidUrl {
            reason: format!("Use at most {MAX_URL_LENGTH} characters."),
        });
    }
    if trimmed.contains('\0') || trimmed.contains(['\r', '\n']) {
        return Err(ProjectError::InvalidUrl {
            reason: "NUL characters and line breaks are not allowed.".to_owned(),
        });
    }

    let parsed = Url::parse(trimmed).map_err(|error| ProjectError::InvalidUrl {
        reason: error.to_string(),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ProjectError::InvalidUrl {
            reason: "Only HTTP and HTTPS URLs are allowed.".to_owned(),
        });
    }
    if !parsed.has_host() {
        return Err(ProjectError::InvalidUrl {
            reason: "The URL must include a host.".to_owned(),
        });
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ProjectError::InvalidUrl {
            reason: "Credentials must not be stored in the local URL.".to_owned(),
        });
    }

    Ok(Some(trimmed.to_owned()))
}

fn validate_project_name(project: &Project) -> Result<(), String> {
    if project.name.trim().is_empty() {
        return Err(format!("project '{}' has an empty name", project.id));
    }
    if project.name != project.name.trim() {
        return Err(format!(
            "project '{}' has leading or trailing whitespace in its name",
            project.id
        ));
    }
    Ok(())
}

fn validate_uuid(id: &str, entity: &str) -> Result<(), String> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| format!("{entity} id '{id}' is not a valid UUID"))
}

fn validate_dates(
    entity: &str,
    id: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<(), String> {
    let created_at = DateTime::parse_from_rfc3339(created_at)
        .map_err(|error| format!("{entity} '{id}' has an invalid creation date: {error}"))?;
    let updated_at = DateTime::parse_from_rfc3339(updated_at)
        .map_err(|error| format!("{entity} '{id}' has an invalid modification date: {error}"))?;

    if updated_at < created_at {
        return Err(format!(
            "{entity} '{id}' was modified before it was created"
        ));
    }
    Ok(())
}
