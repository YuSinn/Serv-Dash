use super::{
    DetectedServiceSkipKind, DetectedServiceSubmission, PersistedData, Project, ProjectStore,
    ServiceInput, DATA_VERSION,
};
use crate::projects::error::ProjectError;
use serde_json::{json, Value};
use std::collections::HashSet;
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

fn input_with_command(name: &str, directory: &str, command: &str) -> ServiceInput {
    let mut service = input(name, directory);
    service.command = command.to_owned();
    service
}

fn submission(stable_id: &str, service: ServiceInput) -> DetectedServiceSubmission {
    DetectedServiceSubmission {
        stable_id: stable_id.to_owned(),
        service,
    }
}

fn temporary_files(data_file: &Path) -> Vec<PathBuf> {
    fs::read_dir(
        data_file
            .parent()
            .expect("data file should have a parent directory"),
    )
    .expect("data directory should be readable")
    .filter_map(|entry| {
        let path = entry.expect("directory entry should be readable").path();
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".tmp"))
            .then_some(path)
    })
    .collect()
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
fn empty_detected_batch_returns_current_services_without_writing() {
    let setup = Setup::new("detected-empty");
    let existing = setup
        .store
        .add_service(
            &setup.project.id,
            &input_with_command("Existing", ".", "existing command"),
        )
        .expect("existing service should be added");
    let before = fs::read(&setup.data_file).expect("data file should exist");

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &[])
        .expect("empty batch should succeed");

    assert!(result.added.is_empty());
    assert!(result.skipped.is_empty());
    assert_eq!(result.services, existing);
    assert_eq!(
        fs::read(&setup.data_file).expect("data file should remain"),
        before
    );
    assert!(temporary_files(&setup.data_file).is_empty());
}

#[test]
fn valid_detected_service_keeps_correlation_id_and_persists_backend_service() {
    let setup = Setup::new("detected-valid");
    let stable_id = "__opaque stable id__";
    let mut service_input = input("Detected", ".");
    service_input.expected_port = Some(65_535);
    service_input.local_url = Some("https://localhost:8443/path".to_owned());

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &[submission(stable_id, service_input)])
        .expect("valid detected service should be added");

    assert_eq!(result.added.len(), 1);
    assert_eq!(result.added[0].stable_id, stable_id);
    assert_ne!(result.added[0].service.id, stable_id);
    assert!(Uuid::parse_str(&result.added[0].service.id).is_ok());
    assert_eq!(result.added[0].service.expected_port, Some(65_535));
    assert_eq!(
        result.added[0].service.local_url.as_deref(),
        Some("https://localhost:8443/path")
    );
    assert_eq!(result.services, vec![result.added[0].service.clone()]);

    let recovered = ProjectStore::new(setup.data_file.clone())
        .list_services(&setup.project.id)
        .expect("detected service should reload");
    assert_eq!(recovered, result.services);
    let persisted = fs::read_to_string(&setup.data_file).expect("data file should be readable");
    assert!(!persisted.contains(stable_id));

    let serialized = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(serialized["added"][0]["stableId"], stable_id);
    assert!(serialized["added"][0]["service"].get("stableId").is_none());
}

#[test]
fn valid_detected_batch_preserves_input_order_and_uses_unique_backend_ids() {
    let setup = Setup::new("detected-many");
    let submissions = vec![
        submission("stable-one", input_with_command("One", ".", "run one")),
        submission("stable-two", input_with_command("Two", ".", "run two")),
        submission(
            "stable-three",
            input_with_command("Three", ".", "run three"),
        ),
    ];

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &submissions)
        .expect("valid batch should succeed");

    assert_eq!(
        result
            .added
            .iter()
            .map(|item| item.stable_id.as_str())
            .collect::<Vec<_>>(),
        vec!["stable-one", "stable-two", "stable-three"]
    );
    assert_eq!(
        result
            .services
            .iter()
            .map(|service| service.name.as_str())
            .collect::<Vec<_>>(),
        vec!["One", "Two", "Three"]
    );
    let ids: HashSet<&str> = result
        .added
        .iter()
        .map(|item| item.service.id.as_str())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| Uuid::parse_str(id).is_ok()));

    let recovered = ProjectStore::new(setup.data_file.clone())
        .list_services(&setup.project.id)
        .expect("batch should reload");
    assert_eq!(recovered, result.services);
}

#[test]
fn partial_detected_batch_persists_valid_items_and_preserves_result_order() {
    let setup = Setup::new("detected-partial");
    let mut invalid = input_with_command("Invalid", ".", "");
    invalid.local_url = None;
    let submissions = vec![
        submission("first", input_with_command("First", ".", "run first")),
        submission("invalid", invalid),
        submission(
            "duplicate-name",
            input_with_command("fIrSt", ".", "different command"),
        ),
        submission("last", input_with_command("Last", ".", "run last")),
    ];

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &submissions)
        .expect("partial batch should succeed");

    assert_eq!(
        result
            .added
            .iter()
            .map(|item| item.stable_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "last"]
    );
    assert_eq!(
        result
            .skipped
            .iter()
            .map(|item| (item.stable_id.as_str(), item.kind))
            .collect::<Vec<_>>(),
        vec![
            ("invalid", DetectedServiceSkipKind::InvalidCommand),
            (
                "duplicate-name",
                DetectedServiceSkipKind::DuplicateBatchName
            ),
        ]
    );
    let added_ids: HashSet<&str> = result
        .added
        .iter()
        .map(|item| item.stable_id.as_str())
        .collect();
    assert!(result
        .skipped
        .iter()
        .all(|item| !added_ids.contains(item.stable_id.as_str())));

    let recovered = ProjectStore::new(setup.data_file.clone())
        .list_services(&setup.project.id)
        .expect("partial batch should reload");
    assert_eq!(
        recovered
            .iter()
            .map(|service| service.name.as_str())
            .collect::<Vec<_>>(),
        vec!["First", "Last"]
    );
    let serialized = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(serialized["skipped"][0]["kind"], "invalidCommand");
    assert_eq!(serialized["skipped"][1]["kind"], "duplicateBatchName");
}

#[test]
fn detected_batch_rejects_existing_name_case_insensitively() {
    let setup = Setup::new("detected-existing-name");
    setup
        .store
        .add_service(
            &setup.project.id,
            &input_with_command("Existing", ".", "existing command"),
        )
        .expect("existing service should be added");
    let before = fs::read(&setup.data_file).expect("data file should exist");

    let result = setup
        .store
        .add_detected_services(
            &setup.project.id,
            &[submission(
                "duplicate",
                input_with_command("eXiStInG", ".", "different command"),
            )],
        )
        .expect("duplicate should be skipped");

    assert!(result.added.is_empty());
    assert_eq!(
        result.skipped[0].kind,
        DetectedServiceSkipKind::DuplicateExistingName
    );
    assert_eq!(
        fs::read(&setup.data_file).expect("data file should remain"),
        before
    );
}

#[test]
fn detected_functional_duplicate_ignores_directory_case_but_not_command_case() {
    let setup = Setup::new("detected-existing-functional");
    let root = Path::new(&setup.project.root_path);
    for directory in ["Apps/Api", "apps/api", "APPS/API"] {
        fs::create_dir_all(root.join(directory)).expect("case variant directory should exist");
    }
    setup
        .store
        .add_service(
            &setup.project.id,
            &input_with_command("Existing", "Apps/Api", "npm run -- dev"),
        )
        .expect("existing service should be added");
    let submissions = vec![
        submission(
            "functional-duplicate",
            input_with_command("Functional duplicate", "apps/api", "npm run -- dev"),
        ),
        submission(
            "different-command-case",
            input_with_command("Different command case", "APPS/API", "NPM run -- dev"),
        ),
    ];

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &submissions)
        .expect("batch should classify functional duplicates");

    assert_eq!(
        result.skipped[0].kind,
        DetectedServiceSkipKind::DuplicateExistingWorkingDirectoryCommand
    );
    assert_eq!(result.added[0].stable_id, "different-command-case");
}

#[test]
fn detected_batch_uses_first_valid_unique_name_and_function() {
    let setup = Setup::new("detected-batch-duplicates");
    let submissions = vec![
        submission("first", input_with_command("First", ".", "first command")),
        submission(
            "duplicate-name",
            input_with_command("fIrSt", ".", "different command"),
        ),
        submission(
            "second",
            input_with_command("Second", ".", "shared command"),
        ),
        submission(
            "duplicate-function",
            input_with_command("Third", ".", "shared command"),
        ),
    ];

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &submissions)
        .expect("batch duplicates should be skipped");

    assert_eq!(
        result
            .added
            .iter()
            .map(|item| item.stable_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
    assert_eq!(
        result
            .skipped
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        vec![
            DetectedServiceSkipKind::DuplicateBatchName,
            DetectedServiceSkipKind::DuplicateBatchWorkingDirectoryCommand,
        ]
    );
}

#[test]
fn all_invalid_detected_services_are_classified_without_writing() {
    let setup = Setup::new("detected-all-invalid");
    let before = fs::read(&setup.data_file).expect("data file should exist");
    let mut invalid_command = input("Invalid command", ".");
    invalid_command.command.clear();
    let mut invalid_port = input("Invalid port", ".");
    invalid_port.expected_port = Some(0);
    let mut invalid_url = input("Invalid URL", ".");
    invalid_url.local_url = Some("ftp://localhost/files".to_owned());
    let submissions = vec![
        submission("invalid-name", input("", ".")),
        submission("invalid-directory", input("Invalid directory", "missing")),
        submission("invalid-command", invalid_command),
        submission("invalid-port", invalid_port),
        submission("invalid-url", invalid_url),
    ];

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &submissions)
        .expect("validation errors should be skipped");

    assert!(result.added.is_empty());
    assert!(result.services.is_empty());
    assert_eq!(
        result
            .skipped
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        vec![
            DetectedServiceSkipKind::InvalidServiceName,
            DetectedServiceSkipKind::InvalidWorkingDirectory,
            DetectedServiceSkipKind::InvalidCommand,
            DetectedServiceSkipKind::InvalidExpectedPort,
            DetectedServiceSkipKind::InvalidLocalUrl,
        ]
    );
    assert!(result.skipped.iter().all(|item| !item.message.is_empty()));
    assert_eq!(
        fs::read(&setup.data_file).expect("data file should remain"),
        before
    );
    assert!(temporary_files(&setup.data_file).is_empty());
}

#[test]
fn invalid_detected_service_does_not_reserve_batch_keys() {
    let setup = Setup::new("detected-invalid-reservation");
    let mut invalid = input_with_command("Shared", ".", "shared command");
    invalid.expected_port = Some(0);
    let valid = input_with_command("shared", ".", "shared command");

    let result = setup
        .store
        .add_detected_services(
            &setup.project.id,
            &[submission("invalid", invalid), submission("valid", valid)],
        )
        .expect("valid service after invalid one should be accepted");

    assert_eq!(result.added[0].stable_id, "valid");
    assert_eq!(
        result.skipped[0].kind,
        DetectedServiceSkipKind::InvalidExpectedPort
    );
}

#[test]
fn existing_duplicates_do_not_reserve_new_batch_keys() {
    let setup = Setup::new("detected-existing-reservation");
    setup
        .store
        .add_service(
            &setup.project.id,
            &input_with_command("Existing", ".", "existing command"),
        )
        .expect("existing service should be added");
    let submissions = vec![
        submission(
            "duplicate-name",
            input_with_command("existing", ".", "new functional key"),
        ),
        submission(
            "after-name",
            input_with_command("After name", ".", "new functional key"),
        ),
        submission(
            "duplicate-function",
            input_with_command("Reserved only if bug", ".", "existing command"),
        ),
        submission(
            "after-function",
            input_with_command("reserved only if bug", ".", "fresh command"),
        ),
    ];

    let result = setup
        .store
        .add_detected_services(&setup.project.id, &submissions)
        .expect("existing duplicates should not reserve batch keys");

    assert_eq!(
        result
            .added
            .iter()
            .map(|item| item.stable_id.as_str())
            .collect::<Vec<_>>(),
        vec!["after-name", "after-function"]
    );
    assert_eq!(
        result
            .skipped
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        vec![
            DetectedServiceSkipKind::DuplicateExistingName,
            DetectedServiceSkipKind::DuplicateExistingWorkingDirectoryCommand,
        ]
    );
}

#[test]
fn missing_project_is_a_global_error_and_does_not_write() {
    let setup = Setup::new("detected-missing-project");
    let before = fs::read(&setup.data_file).expect("data file should exist");

    assert!(matches!(
        setup.store.add_detected_services(
            &Uuid::new_v4().to_string(),
            &[submission("one", input("One", "."))],
        ),
        Err(ProjectError::ProjectNotFound { .. })
    ));
    assert_eq!(
        fs::read(&setup.data_file).expect("data file should remain"),
        before
    );
}

#[test]
fn historical_functional_duplicates_still_load_and_block_another_detected_service() {
    let setup = Setup::new("detected-historical-functional");
    setup
        .store
        .add_service(
            &setup.project.id,
            &input_with_command("Historical one", ".", "same command"),
        )
        .expect("first historical service should be added");
    setup
        .store
        .add_service(
            &setup.project.id,
            &input_with_command("Historical two", ".", "same command"),
        )
        .expect("manual add should keep allowing functional duplicates");
    assert_eq!(
        ProjectStore::new(setup.data_file.clone())
            .list_services(&setup.project.id)
            .expect("historical duplicates should load")
            .len(),
        2
    );
    let before = fs::read(&setup.data_file).expect("data file should exist");

    let result = setup
        .store
        .add_detected_services(
            &setup.project.id,
            &[submission(
                "third",
                input_with_command("Historical three", ".", "same command"),
            )],
        )
        .expect("third functional duplicate should be skipped");

    assert!(result.added.is_empty());
    assert_eq!(
        result.skipped[0].kind,
        DetectedServiceSkipKind::DuplicateExistingWorkingDirectoryCommand
    );
    assert_eq!(result.services.len(), 2);
    assert_eq!(
        fs::read(&setup.data_file).expect("data file should remain"),
        before
    );
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
