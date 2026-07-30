mod commands;
mod error;
mod model;
mod paths;
mod store;

pub(crate) use commands::{
    add_project, add_service, list_projects, list_services, open_project_folder, remove_project,
    remove_service, rename_project, resolve_service_directory, update_service,
};
pub(crate) use error::{CommandError, ProjectError};
#[cfg(test)]
pub(crate) use model::ServiceInput;
pub(crate) use model::{ServiceLaunchSpec, ServiceStartPreview};
pub(crate) use store::ProjectStore;

use std::sync::{Mutex, MutexGuard};

pub(crate) struct ProjectsState {
    store: Mutex<ProjectStore>,
}

impl ProjectsState {
    pub fn new(store: ProjectStore) -> Self {
        Self {
            store: Mutex::new(store),
        }
    }

    pub(crate) fn store(&self) -> Result<MutexGuard<'_, ProjectStore>, ProjectError> {
        self.store
            .lock()
            .map_err(|_| ProjectError::StateUnavailable)
    }
}
