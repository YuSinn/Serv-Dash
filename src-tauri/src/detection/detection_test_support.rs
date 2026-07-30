use super::model::{DetectionResult, DetectionWarningKind};
use super::scanner::{scan_project, ScanConfig};
use crate::projects::{ProjectStore, ProjectsState};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    pub fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("server-dashboard-detection-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp fixture should be created");
        Self { root }
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        if relative.is_empty() {
            self.root.clone()
        } else {
            self.root
                .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR))
        }
    }

    pub fn mkdir(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).expect("fixture directory should be created");
        path
    }

    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        self.write_bytes(relative, contents.as_bytes())
    }

    pub fn write_bytes(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(&path, contents).expect("fixture file should be written");
        path
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn scan(fixture: &TempFixture, project_id: &str, config: ScanConfig) -> DetectionResult {
    scan_project(project_id, &fixture.path(""), config).expect("scan should succeed")
}

pub(super) fn warning_count(result: &DetectionResult, kind: DetectionWarningKind) -> usize {
    result
        .warnings
        .iter()
        .filter(|warning| warning.kind == kind)
        .count()
}

pub(super) fn registered_state(fixture: &TempFixture) -> (ProjectsState, String, PathBuf) {
    let project_root = fixture.mkdir("project");
    let data_file = fixture.path("data/projects.json");
    let store = ProjectStore::new(data_file.clone());
    let projects = store
        .add_project(project_root.to_string_lossy().as_ref(), Some("Fixture"))
        .expect("project should register");
    (ProjectsState::new(store), projects[0].id.clone(), data_file)
}

pub(super) fn suggestion<'a>(
    result: &'a DetectionResult,
    command: &str,
) -> &'a super::model::ServiceSuggestion {
    result
        .suggestions
        .iter()
        .find(|suggestion| suggestion.command == command)
        .expect("suggestion should exist")
}

pub(super) fn assert_path_absent(path: &Path) {
    assert!(!path.exists(), "{} should not exist", path.display());
}
