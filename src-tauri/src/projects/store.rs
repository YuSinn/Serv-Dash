use crate::projects::error::ProjectError;
use crate::projects::model::{
    normalize_command, normalize_expected_port, normalize_local_url, normalize_service_name,
    service_name_key, AddDetectedServicesResult, AddedDetectedService, DetectedServiceSkipKind,
    DetectedServiceSubmission, LegacyPersistedData, PersistedData, Project, ServiceDefinition,
    ServiceInput, ServiceLaunchSpec, SkippedDetectedService, DATA_VERSION, LEGACY_DATA_VERSION,
};
use crate::projects::paths::{
    canonical_service_directory, normalize_existing_directory, normalize_service_directory,
    path_key, relative_service_directory,
};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(crate) struct ProjectStore {
    data_file: PathBuf,
}

impl ProjectStore {
    pub fn new(data_file: PathBuf) -> Self {
        Self { data_file }
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, ProjectError> {
        Ok(self.load()?.projects)
    }

    pub fn add_project(
        &self,
        root_path: &str,
        requested_name: Option<&str>,
    ) -> Result<Vec<Project>, ProjectError> {
        let normalized_path = normalize_existing_directory(root_path)?;
        let mut data = self.load()?;
        let normalized_key = path_key(&normalized_path);

        if data
            .projects
            .iter()
            .any(|project| path_key(Path::new(&project.root_path)) == normalized_key)
        {
            return Err(ProjectError::DuplicatePath {
                path: normalized_path,
            });
        }

        let name = project_name(requested_name, &normalized_path)?;
        let root_path = path_to_string(normalized_path)?;
        let now = current_timestamp();

        data.projects.push(Project {
            id: Uuid::new_v4().to_string(),
            name,
            root_path,
            created_at: now.clone(),
            updated_at: now,
            services: Vec::new(),
        });

        self.save(&data)?;
        Ok(data.projects)
    }

    pub fn rename_project(
        &self,
        project_id: &str,
        requested_name: &str,
    ) -> Result<Vec<Project>, ProjectError> {
        let name = validate_project_name(requested_name)?;
        let mut data = self.load()?;
        let project = find_project_mut(&mut data, project_id)?;

        if project.name == name {
            return Ok(data.projects);
        }

        project.name = name;
        project.updated_at = current_timestamp();
        self.save(&data)?;
        Ok(data.projects)
    }

    pub fn remove_project(&self, project_id: &str) -> Result<Vec<Project>, ProjectError> {
        let mut data = self.load()?;
        let original_length = data.projects.len();
        data.projects.retain(|project| project.id != project_id);

        if data.projects.len() == original_length {
            return Err(ProjectError::ProjectNotFound {
                project_id: project_id.to_owned(),
            });
        }

        self.save(&data)?;
        Ok(data.projects)
    }

    pub fn registered_path(&self, project_id: &str) -> Result<PathBuf, ProjectError> {
        let data = self.load()?;
        let project = find_project(&data, project_id)?;
        Ok(PathBuf::from(&project.root_path))
    }

    pub fn list_services(&self, project_id: &str) -> Result<Vec<ServiceDefinition>, ProjectError> {
        let data = self.load()?;
        Ok(find_project(&data, project_id)?.services.clone())
    }

    pub fn ensure_service_exists(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<(), ProjectError> {
        let data = self.load()?;
        let project = find_project(&data, project_id)?;
        find_service(project, project_id, service_id)?;
        Ok(())
    }

    pub fn prepare_service_start(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<ServiceLaunchSpec, ProjectError> {
        let data = self.load()?;
        let project = find_project(&data, project_id)?;
        let service = find_service(project, project_id, service_id)?;

        let working_directory =
            canonical_service_directory(Path::new(&project.root_path), &service.working_directory)?;
        let command = normalize_command(&service.command)?;

        Ok(ServiceLaunchSpec {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            service_id: service.id.clone(),
            service_name: service.name.clone(),
            working_directory,
            command,
        })
    }

    pub fn add_service(
        &self,
        project_id: &str,
        input: &ServiceInput,
    ) -> Result<Vec<ServiceDefinition>, ProjectError> {
        let mut data = self.load()?;
        let project = find_project(&data, project_id)?;
        let prepared = prepare_service(&project.root_path, input)?;
        reject_duplicate_service_name(project, &prepared.name, None)?;

        let now = current_timestamp();
        let project = find_project_mut(&mut data, project_id)?;
        project.services.push(ServiceDefinition {
            id: Uuid::new_v4().to_string(),
            name: prepared.name,
            working_directory: prepared.working_directory,
            command: prepared.command,
            expected_port: prepared.expected_port,
            local_url: prepared.local_url,
            created_at: now.clone(),
            updated_at: now.clone(),
        });
        project.updated_at = now;
        let services = project.services.clone();

        self.save(&data)?;
        Ok(services)
    }

    pub fn add_detected_services(
        &self,
        project_id: &str,
        submissions: &[DetectedServiceSubmission],
    ) -> Result<AddDetectedServicesResult, ProjectError> {
        let mut data = self.load()?;
        let project = find_project_mut(&mut data, project_id)?;
        let root_path = project.root_path.clone();
        let existing_names: HashSet<String> = project
            .services
            .iter()
            .map(|service| service_name_key(&service.name))
            .collect();
        let existing_service_keys: HashSet<(String, String)> = project
            .services
            .iter()
            .map(|service| service_function_key(&service.working_directory, &service.command))
            .collect();
        let mut batch_names = HashSet::new();
        let mut batch_service_keys = HashSet::new();
        let mut added = Vec::new();
        let mut skipped = Vec::new();

        for submission in submissions {
            let submitted_name = submission.service.name.trim().to_owned();
            let prepared = match prepare_service(&root_path, &submission.service) {
                Ok(prepared) => prepared,
                Err(error) => {
                    let Some(kind) = detected_service_validation_kind(&error) else {
                        return Err(error);
                    };
                    skipped.push(SkippedDetectedService {
                        stable_id: submission.stable_id.clone(),
                        name: submitted_name,
                        kind,
                        message: error.to_string(),
                    });
                    continue;
                }
            };

            let name_key = service_name_key(&prepared.name);
            let service_key = service_function_key(&prepared.working_directory, &prepared.command);
            let duplicate = if existing_names.contains(&name_key) {
                Some((
                    DetectedServiceSkipKind::DuplicateExistingName,
                    "A service with this name already exists.",
                ))
            } else if existing_service_keys.contains(&service_key) {
                Some((
                    DetectedServiceSkipKind::DuplicateExistingWorkingDirectoryCommand,
                    "A service with this working directory and command already exists.",
                ))
            } else if batch_names.contains(&name_key) {
                Some((
                    DetectedServiceSkipKind::DuplicateBatchName,
                    "Another selected service uses this name.",
                ))
            } else if batch_service_keys.contains(&service_key) {
                Some((
                    DetectedServiceSkipKind::DuplicateBatchWorkingDirectoryCommand,
                    "Another selected service uses this working directory and command.",
                ))
            } else {
                None
            };

            if let Some((kind, message)) = duplicate {
                skipped.push(SkippedDetectedService {
                    stable_id: submission.stable_id.clone(),
                    name: prepared.name,
                    kind,
                    message: message.to_owned(),
                });
                continue;
            }

            let now = current_timestamp();
            let service = ServiceDefinition {
                id: Uuid::new_v4().to_string(),
                name: prepared.name,
                working_directory: prepared.working_directory,
                command: prepared.command,
                expected_port: prepared.expected_port,
                local_url: prepared.local_url,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            batch_names.insert(name_key);
            batch_service_keys.insert(service_key);
            project.services.push(service.clone());
            project.updated_at = now;
            added.push(AddedDetectedService {
                stable_id: submission.stable_id.clone(),
                service,
            });
        }

        let services = project.services.clone();
        if !added.is_empty() {
            self.save(&data)?;
        }

        Ok(AddDetectedServicesResult {
            added,
            skipped,
            services,
        })
    }

    pub fn update_service(
        &self,
        project_id: &str,
        service_id: &str,
        input: &ServiceInput,
    ) -> Result<Vec<ServiceDefinition>, ProjectError> {
        let mut data = self.load()?;
        let project = find_project(&data, project_id)?;
        if !project
            .services
            .iter()
            .any(|service| service.id == service_id)
        {
            return Err(ProjectError::ServiceNotFound {
                project_id: project_id.to_owned(),
                service_id: service_id.to_owned(),
            });
        }

        let prepared = prepare_service(&project.root_path, input)?;
        reject_duplicate_service_name(project, &prepared.name, Some(service_id))?;

        let now = current_timestamp();
        let project = find_project_mut(&mut data, project_id)?;
        let service = project
            .services
            .iter_mut()
            .find(|service| service.id == service_id)
            .ok_or_else(|| ProjectError::ServiceNotFound {
                project_id: project_id.to_owned(),
                service_id: service_id.to_owned(),
            })?;

        service.name = prepared.name;
        service.working_directory = prepared.working_directory;
        service.command = prepared.command;
        service.expected_port = prepared.expected_port;
        service.local_url = prepared.local_url;
        service.updated_at = now.clone();
        project.updated_at = now;
        let services = project.services.clone();

        self.save(&data)?;
        Ok(services)
    }

    pub fn remove_service(
        &self,
        project_id: &str,
        service_id: &str,
    ) -> Result<Vec<ServiceDefinition>, ProjectError> {
        let mut data = self.load()?;
        let project = find_project_mut(&mut data, project_id)?;
        let original_length = project.services.len();
        project.services.retain(|service| service.id != service_id);

        if project.services.len() == original_length {
            return Err(ProjectError::ServiceNotFound {
                project_id: project_id.to_owned(),
                service_id: service_id.to_owned(),
            });
        }

        project.updated_at = current_timestamp();
        let services = project.services.clone();
        self.save(&data)?;
        Ok(services)
    }

    pub fn resolve_service_directory(
        &self,
        project_id: &str,
        selected_path: &str,
    ) -> Result<String, ProjectError> {
        let data = self.load()?;
        let project = find_project(&data, project_id)?;
        relative_service_directory(Path::new(&project.root_path), selected_path)
    }

    fn load(&self) -> Result<PersistedData, ProjectError> {
        let bytes = match fs::read(&self.data_file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(PersistedData::empty());
            }
            Err(error) => {
                return Err(ProjectError::Persistence {
                    action: "read",
                    path: self.data_file.clone(),
                    reason: error.to_string(),
                });
            }
        };

        let value: Value =
            serde_json::from_slice(&bytes).map_err(|error| ProjectError::CorruptData {
                path: self.data_file.clone(),
                reason: error.to_string(),
            })?;

        let version = value
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| ProjectError::InvalidData {
                path: self.data_file.clone(),
                reason: "the required numeric 'version' field is missing".to_owned(),
            })?;

        match version {
            version if version == u64::from(DATA_VERSION) => {
                let data: PersistedData =
                    serde_json::from_value(value).map_err(|error| ProjectError::InvalidData {
                        path: self.data_file.clone(),
                        reason: error.to_string(),
                    })?;
                self.validate_loaded_data(data)
            }
            version if version == u64::from(LEGACY_DATA_VERSION) => {
                let legacy: LegacyPersistedData =
                    serde_json::from_value(value).map_err(|error| ProjectError::InvalidData {
                        path: self.data_file.clone(),
                        reason: error.to_string(),
                    })?;
                let migrated = PersistedData::migrate_from_v1(legacy).map_err(|reason| {
                    ProjectError::InvalidData {
                        path: self.data_file.clone(),
                        reason,
                    }
                })?;
                self.save(&migrated)?;
                Ok(migrated)
            }
            found => Err(ProjectError::UnsupportedVersion {
                path: self.data_file.clone(),
                expected: DATA_VERSION,
                found,
            }),
        }
    }

    fn validate_loaded_data(&self, data: PersistedData) -> Result<PersistedData, ProjectError> {
        data.validate()
            .map_err(|reason| ProjectError::InvalidData {
                path: self.data_file.clone(),
                reason,
            })?;
        Ok(data)
    }

    fn save(&self, data: &PersistedData) -> Result<(), ProjectError> {
        data.validate()
            .map_err(|reason| ProjectError::InvalidData {
                path: self.data_file.clone(),
                reason,
            })?;

        let mut bytes =
            serde_json::to_vec_pretty(data).map_err(|error| ProjectError::Persistence {
                action: "serialized",
                path: self.data_file.clone(),
                reason: error.to_string(),
            })?;
        bytes.push(b'\n');

        let parent = self
            .data_file
            .parent()
            .ok_or_else(|| ProjectError::Persistence {
                action: "saved",
                path: self.data_file.clone(),
                reason: "the data file has no parent directory".to_owned(),
            })?;

        fs::create_dir_all(parent).map_err(|error| ProjectError::Persistence {
            action: "saved",
            path: self.data_file.clone(),
            reason: error.to_string(),
        })?;

        write_atomically(&self.data_file, &bytes).map_err(|error| ProjectError::Persistence {
            action: "saved",
            path: self.data_file.clone(),
            reason: error.to_string(),
        })
    }
}

struct PreparedService {
    name: String,
    working_directory: String,
    command: String,
    expected_port: Option<u16>,
    local_url: Option<String>,
}

fn prepare_service(root_path: &str, input: &ServiceInput) -> Result<PreparedService, ProjectError> {
    Ok(PreparedService {
        name: normalize_service_name(&input.name)?,
        working_directory: normalize_service_directory(
            Path::new(root_path),
            &input.working_directory,
        )?,
        command: normalize_command(&input.command)?,
        expected_port: normalize_expected_port(input.expected_port)?,
        local_url: normalize_local_url(input.local_url.as_deref())?,
    })
}

fn detected_service_validation_kind(error: &ProjectError) -> Option<DetectedServiceSkipKind> {
    match error {
        ProjectError::InvalidServiceName { .. } => {
            Some(DetectedServiceSkipKind::InvalidServiceName)
        }
        ProjectError::InvalidWorkingDirectory { .. }
        | ProjectError::WorkingDirectoryNotFound { .. }
        | ProjectError::WorkingDirectoryNotDirectory { .. }
        | ProjectError::WorkingDirectoryOutsideProject { .. }
        | ProjectError::WorkingDirectoryEscapesProject { .. } => {
            Some(DetectedServiceSkipKind::InvalidWorkingDirectory)
        }
        ProjectError::InvalidCommand { .. } => Some(DetectedServiceSkipKind::InvalidCommand),
        ProjectError::InvalidPort { .. } => Some(DetectedServiceSkipKind::InvalidExpectedPort),
        ProjectError::InvalidUrl { .. } => Some(DetectedServiceSkipKind::InvalidLocalUrl),
        _ => None,
    }
}

fn service_function_key(working_directory: &str, command: &str) -> (String, String) {
    (working_directory.to_lowercase(), command.to_owned())
}

fn reject_duplicate_service_name(
    project: &Project,
    requested_name: &str,
    excluded_service_id: Option<&str>,
) -> Result<(), ProjectError> {
    let requested_key = service_name_key(requested_name);
    if project.services.iter().any(|service| {
        Some(service.id.as_str()) != excluded_service_id
            && service_name_key(&service.name) == requested_key
    }) {
        return Err(ProjectError::DuplicateServiceName {
            name: requested_name.to_owned(),
        });
    }
    Ok(())
}

fn find_project<'a>(
    data: &'a PersistedData,
    project_id: &str,
) -> Result<&'a Project, ProjectError> {
    data.projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| ProjectError::ProjectNotFound {
            project_id: project_id.to_owned(),
        })
}

fn find_service<'a>(
    project: &'a Project,
    project_id: &str,
    service_id: &str,
) -> Result<&'a ServiceDefinition, ProjectError> {
    project
        .services
        .iter()
        .find(|service| service.id == service_id)
        .ok_or_else(|| ProjectError::ServiceNotFound {
            project_id: project_id.to_owned(),
            service_id: service_id.to_owned(),
        })
}

fn find_project_mut<'a>(
    data: &'a mut PersistedData,
    project_id: &str,
) -> Result<&'a mut Project, ProjectError> {
    data.projects
        .iter_mut()
        .find(|project| project.id == project_id)
        .ok_or_else(|| ProjectError::ProjectNotFound {
            project_id: project_id.to_owned(),
        })
}

fn project_name(requested_name: Option<&str>, root_path: &Path) -> Result<String, ProjectError> {
    if let Some(name) = requested_name.filter(|name| !name.trim().is_empty()) {
        return validate_project_name(name);
    }

    let folder_name = root_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| root_path.to_string_lossy().into_owned());

    validate_project_name(&folder_name)
}

fn validate_project_name(name: &str) -> Result<String, ProjectError> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(ProjectError::InvalidName {
            reason: "The name cannot be empty.".to_owned(),
        });
    }

    Ok(trimmed_name.to_owned())
}

fn path_to_string(path: PathBuf) -> Result<String, ProjectError> {
    path.into_os_string()
        .into_string()
        .map_err(|_| ProjectError::InvalidPath {
            reason: "The selected folder path cannot be represented as text.".to_owned(),
        })
}

fn current_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn write_atomically(destination: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("projects.json");
    let temporary_path = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    let write_result = (|| {
        let mut temporary_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        replace_file(&temporary_path, destination)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }

    write_result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();

    // Both paths are in the same directory, so MoveFileExW replaces the target atomically.
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
