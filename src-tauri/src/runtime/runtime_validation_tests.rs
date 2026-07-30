use crate::projects::ProjectError;
use crate::runtime::test_support::{long_command, Setup};
use std::fs;
use uuid::Uuid;

#[test]
fn start_validation_rejects_a_missing_project() {
    let setup = Setup::new("missing-project", "exit /b 0", ".");
    let result = setup
        .store
        .prepare_service_start(&Uuid::new_v4().to_string(), &Uuid::new_v4().to_string());

    assert!(matches!(result, Err(ProjectError::ProjectNotFound { .. })));
}

#[test]
fn start_validation_rejects_a_missing_service() {
    let setup = Setup::new("missing-service", "exit /b 0", ".");
    let result = setup
        .store
        .prepare_service_start(&setup.project_id, &Uuid::new_v4().to_string());

    assert!(matches!(result, Err(ProjectError::ServiceNotFound { .. })));
}

#[test]
fn working_directory_is_canonicalized_again_before_start() {
    let setup = Setup::new("revalidate-directory", long_command(), "work");
    let launch = setup
        .store
        .prepare_service_start(&setup.project_id, &setup.service_id)
        .expect("working directory should still be valid");
    let expected = fs::canonicalize(setup.root.join("work"))
        .expect("directory should canonicalize")
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_owned();

    assert_eq!(launch.working_directory.to_string_lossy(), expected);
}

#[test]
fn start_is_rejected_if_the_saved_directory_was_removed() {
    let setup = Setup::new("removed-directory", long_command(), "work");
    fs::remove_dir(setup.root.join("work")).expect("working directory should be removed");

    assert!(matches!(
        setup
            .store
            .prepare_service_start(&setup.project_id, &setup.service_id),
        Err(ProjectError::WorkingDirectoryNotFound { .. })
    ));
}
