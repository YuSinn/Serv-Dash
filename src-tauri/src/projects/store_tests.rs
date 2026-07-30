use super::{PersistedData, Project, ProjectStore, ServiceInput, DATA_VERSION};
use crate::projects::error::ProjectError;
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("server-dashboard-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("temporary directory should be created");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Setup {
    temp: TempDir,
    store: ProjectStore,
    project: Project,
    data_file: PathBuf,
}

impl Setup {
    fn new(name: &str) -> Self {
        let temp = TempDir::new(name);
        let root = temp.0.join("project");
        fs::create_dir(&root).expect("project directory should be created");
        let data_file = temp.0.join("data").join("projects.json");
        let store = ProjectStore::new(data_file.clone());
        let project = store
            .add_project(root.to_string_lossy().as_ref(), Some("Project"))
            .expect("project should be added")
            .remove(0);
        Self {
            temp,
            store,
            project,
            data_file,
        }
    }
}

fn input(name: &str, directory: &str) -> ServiceInput {
    ServiceInput {
        name: name.to_owned(),
        working_directory: directory.to_owned(),
        command: "npm run dev".to_owned(),
        expected_port: Some(3000),
        local_url: Some("http://localhost:3000".to_owned()),
    }
}

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().expect("data file should have a parent"))
        .expect("data directory should be created");
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("JSON should serialize"),
    )
    .expect("JSON should be written");
}

fn legacy_project(root: &Path, id: &str) -> Value {
    json!({
        "version": 1,
        "projects": [{
            "id": id,
            "name": "Preserved name",
            "rootPath": root,
            "createdAt": "2025-01-02T03:04:05.000Z",
            "updatedAt": "2026-06-07T08:09:10.000Z"
        }]
    })
}

#[test]
fn version_one_is_migrated_to_version_two_with_empty_services() {
    let temp = TempDir::new("migration-version");
    let root = temp.0.join("project");
    fs::create_dir(&root).expect("project directory should be created");
    let data_file = temp.0.join("projects.json");
    write_json(
        &data_file,
        &legacy_project(&root, &Uuid::new_v4().to_string()),
    );

    let projects = ProjectStore::new(data_file.clone())
        .list_projects()
        .expect("migration should succeed");
    assert!(projects[0].services.is_empty());
    let saved: Value = serde_json::from_slice(&fs::read(data_file).expect("file should exist"))
        .expect("saved JSON should parse");
    assert_eq!(saved["version"], DATA_VERSION);
    assert_eq!(saved["projects"][0]["services"], json!([]));
}

#[test]
fn migration_preserves_all_project_fields() {
    let temp = TempDir::new("migration-fields");
    let root = temp.0.join("preserved-root");
    fs::create_dir(&root).expect("project directory should be created");
    let data_file = temp.0.join("projects.json");
    let id = Uuid::new_v4().to_string();
    let root_text = root.to_string_lossy().into_owned();
    write_json(&data_file, &legacy_project(&root, &id));

    let project = ProjectStore::new(data_file)
        .list_projects()
        .expect("migration should succeed")
        .remove(0);
    assert_eq!(project.id, id);
    assert_eq!(project.name, "Preserved name");
    assert_eq!(project.root_path, root_text);
    assert_eq!(project.created_at, "2025-01-02T03:04:05.000Z");
    assert_eq!(project.updated_at, "2026-06-07T08:09:10.000Z");
}

#[test]
fn future_version_is_rejected_without_overwriting() {
    let temp = TempDir::new("future-version");
    let data_file = temp.0.join("projects.json");
    let bytes = br#"{"version":3,"projects":[]}"#;
    fs::write(&data_file, bytes).expect("future data should be written");
    let store = ProjectStore::new(data_file.clone());
    assert!(matches!(
        store.list_projects(),
        Err(ProjectError::UnsupportedVersion {
            expected: DATA_VERSION,
            found: 3,
            ..
        })
    ));
    assert_eq!(fs::read(data_file).expect("file should remain"), bytes);
}

#[test]
fn valid_service_is_created_and_dot_is_accepted() {
    let setup = Setup::new("valid-service");
    let service = setup
        .store
        .add_service(&setup.project.id, &input("Client", "."))
        .expect("service should be added")
        .remove(0);
    assert_eq!(service.name, "Client");
    assert_eq!(service.working_directory, ".");
    assert_eq!(service.expected_port, Some(3000));
}

#[test]
fn duplicate_service_name_is_case_insensitive() {
    let setup = Setup::new("duplicate-service");
    setup
        .store
        .add_service(&setup.project.id, &input("Client", "."))
        .expect("first service should be added");
    assert!(matches!(
        setup
            .store
            .add_service(&setup.project.id, &input("cLiEnT", ".")),
        Err(ProjectError::DuplicateServiceName { .. })
    ));
}

#[test]
fn absolute_and_parent_working_directories_are_rejected() {
    let setup = Setup::new("invalid-relative");
    for directory in [&setup.project.root_path, r"..\other"] {
        assert!(matches!(
            setup
                .store
                .add_service(&setup.project.id, &input("Client", directory)),
            Err(ProjectError::InvalidWorkingDirectory { .. })
        ));
    }
}

#[test]
fn folder_outside_project_is_rejected() {
    let setup = Setup::new("outside-service");
    let outside = setup.temp.0.join("outside");
    fs::create_dir(&outside).expect("outside directory should be created");
    assert!(matches!(
        setup
            .store
            .resolve_service_directory(&setup.project.id, outside.to_string_lossy().as_ref()),
        Err(ProjectError::WorkingDirectoryOutsideProject { .. })
    ));
}

#[test]
fn directory_link_escape_is_rejected_when_links_are_available() {
    let setup = Setup::new("link-escape");
    let outside = setup.temp.0.join("outside");
    let link = Path::new(&setup.project.root_path).join("escape");
    fs::create_dir(&outside).expect("outside directory should be created");
    if let Err(error) = create_directory_link(&outside, &link) {
        if error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1314) {
            return;
        }
        panic!("directory link should be created when supported: {error}");
    }
    assert!(matches!(
        setup
            .store
            .add_service(&setup.project.id, &input("Escape", "escape")),
        Err(ProjectError::WorkingDirectoryEscapesProject { .. })
    ));
}

#[test]
fn expected_port_range_is_validated() {
    let setup = Setup::new("ports");
    for (name, port) in [("Zero", 0), ("High", 65_536)] {
        let mut service = input(name, ".");
        service.expected_port = Some(port);
        assert!(matches!(
            setup.store.add_service(&setup.project.id, &service),
            Err(ProjectError::InvalidPort { port: found }) if found == port
        ));
    }
    for (name, port) in [("Minimum", 1), ("Maximum", 65_535)] {
        let mut service = input(name, ".");
        service.expected_port = Some(port);
        assert!(setup.store.add_service(&setup.project.id, &service).is_ok());
    }
}

#[test]
fn http_and_https_urls_are_accepted() {
    let setup = Setup::new("valid-urls");
    for (name, url) in [
        ("HTTP", "http://localhost:3000"),
        ("HTTPS", "https://localhost:8443/path"),
    ] {
        let mut service = input(name, ".");
        service.local_url = Some(url.to_owned());
        assert!(setup.store.add_service(&setup.project.id, &service).is_ok());
    }
}

#[test]
fn non_http_url_scheme_is_rejected() {
    let setup = Setup::new("invalid-url");
    let mut service = input("FTP", ".");
    service.local_url = Some("ftp://localhost/files".to_owned());
    assert!(matches!(
        setup.store.add_service(&setup.project.id, &service),
        Err(ProjectError::InvalidUrl { .. })
    ));
}

#[test]
fn services_are_persisted_and_recovered() {
    let setup = Setup::new("service-persistence");
    let expected = setup
        .store
        .add_service(&setup.project.id, &input("Client", "."))
        .expect("service should be added");
    let recovered = ProjectStore::new(setup.data_file.clone())
        .list_services(&setup.project.id)
        .expect("services should reload");
    assert_eq!(recovered, expected);
}

#[test]
fn service_can_be_edited() {
    let setup = Setup::new("edit-service");
    fs::create_dir(Path::new(&setup.project.root_path).join("api"))
        .expect("API directory should be created");
    let original = setup
        .store
        .add_service(&setup.project.id, &input("Server", "."))
        .expect("service should be added")
        .remove(0);
    let mut edited = input("API", "api");
    edited.command = "cargo run".to_owned();
    edited.expected_port = Some(4000);
    edited.local_url = None;
    let result = setup
        .store
        .update_service(&setup.project.id, &original.id, &edited)
        .expect("service should be updated")
        .remove(0);
    assert_eq!(result.id, original.id);
    assert_eq!(result.created_at, original.created_at);
    assert_eq!(result.name, "API");
    assert_eq!(result.working_directory, "api");
    assert_eq!(result.command, "cargo run");
    assert_eq!(result.expected_port, Some(4000));
    assert_eq!(result.local_url, None);
}

#[test]
fn service_can_be_removed_without_touching_folder() {
    let setup = Setup::new("remove-service");
    let folder = Path::new(&setup.project.root_path).join("client");
    fs::create_dir(&folder).expect("client directory should be created");
    let service = setup
        .store
        .add_service(&setup.project.id, &input("Client", "client"))
        .expect("service should be added")
        .remove(0);
    assert!(setup
        .store
        .remove_service(&setup.project.id, &service.id)
        .expect("service should be removed")
        .is_empty());
    assert!(folder.is_dir());
}

#[test]
fn removing_project_removes_services_but_not_real_files() {
    let setup = Setup::new("remove-project-services");
    let folder = Path::new(&setup.project.root_path).join("server");
    fs::create_dir(&folder).expect("server directory should be created");
    let marker = folder.join("keep.txt");
    fs::write(&marker, "keep").expect("marker should be written");
    setup
        .store
        .add_service(&setup.project.id, &input("Server", "server"))
        .expect("service should be added");
    assert!(setup
        .store
        .remove_project(&setup.project.id)
        .expect("project should be removed")
        .is_empty());
    assert!(marker.is_file());
    assert!(ProjectStore::new(setup.data_file.clone())
        .list_projects()
        .expect("projects should reload")
        .is_empty());
}

#[test]
fn serialization_round_trip_includes_v2_shape() {
    let temp = TempDir::new("serialization");
    let original = PersistedData {
        version: DATA_VERSION,
        projects: vec![Project {
            id: Uuid::new_v4().to_string(),
            name: "Example".to_owned(),
            root_path: temp.0.to_string_lossy().into_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
            services: Vec::new(),
        }],
    };
    let json = serde_json::to_string(&original).expect("data should serialize");
    let restored: PersistedData = serde_json::from_str(&json).expect("data should deserialize");
    assert_eq!(restored, original);
    assert!(restored.validate().is_ok());
}

#[test]
fn missing_and_corrupt_files_keep_safe_behavior() {
    let temp = TempDir::new("safe-load");
    let data_file = temp.0.join("data").join("projects.json");
    let store = ProjectStore::new(data_file.clone());
    assert!(store
        .list_projects()
        .expect("missing file should load")
        .is_empty());
    assert!(!data_file.exists());

    let corrupt = b"{ not valid json";
    fs::create_dir_all(data_file.parent().expect("file should have a parent"))
        .expect("data directory should be created");
    fs::write(&data_file, corrupt).expect("corrupt data should be written");
    assert!(matches!(
        store.list_projects(),
        Err(ProjectError::CorruptData { .. })
    ));
    assert_eq!(fs::read(data_file).expect("file should remain"), corrupt);
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
