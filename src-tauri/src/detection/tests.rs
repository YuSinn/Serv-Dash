use super::commands::detect_registered_project;
use super::detection_test_support::{
    assert_path_absent, registered_state, scan, suggestion, warning_count, TempFixture,
};
use super::model::{DetectionWarning, DetectionWarningKind, SourceKind};
use super::scanner::{
    scan_project, ScanConfig, MAX_DEPTH, MAX_DIRECTORIES, MAX_PACKAGE_JSON_BYTES,
    MAX_PACKAGE_JSON_FILES, MAX_SUGGESTIONS,
};
use std::fs;
use std::io;

#[cfg(windows)]
use super::scanner::has_reparse_attribute;

#[test]
fn scanner_is_iterative_and_deterministic_across_root_and_nested_directories() {
    let fixture = TempFixture::new();
    fixture.write(
        "package.json",
        r#"{"scripts":{"z":"z","dev":"d","DEV":"D"}}"#,
    );
    fixture.write("beta/package.json", r#"{"scripts":{"serve":"x"}}"#);
    fixture.write("Alpha/deep/package.json", r#"{"scripts":{"start":"x"}}"#);

    let first = scan(&fixture, "project-a", ScanConfig::default());
    let second = scan(&fixture, "project-a", ScanConfig::default());
    let commands: Vec<_> = first
        .suggestions
        .iter()
        .map(|item| item.command.as_str())
        .collect();

    assert_eq!(first, second);
    assert_eq!(
        commands,
        [
            "npm run -- DEV",
            "npm run -- dev",
            "npm run -- z",
            "npm run -- start",
            "npm run -- serve"
        ]
    );
    assert_eq!(first.scanned_directories, 4);
}

#[test]
fn scanner_excludes_only_the_named_initial_directories() {
    let fixture = TempFixture::new();
    for directory in [
        "node_modules",
        ".git",
        "dist",
        "build",
        "target",
        "out",
        "coverage",
        ".next",
        ".turbo",
        "vendor",
        "bin",
        "obj",
        ".cache",
        ".pnpm-store",
        ".yarn",
    ] {
        fixture.write(
            &format!("{directory}/package.json"),
            r#"{"scripts":{"hidden":"x"}}"#,
        );
    }
    fixture.write(".custom/package.json", r#"{"scripts":{"visible":"x"}}"#);

    let result = scan(&fixture, "project-a", ScanConfig::default());
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(result.suggestions[0].source_path, ".custom/package.json");
}

#[test]
fn configured_limits_truncate_once_and_keep_prior_suggestions() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"root":"x"}}"#);
    fixture.write("a/package.json", r#"{"scripts":{"a":"x"}}"#);
    fixture.write("a/b/package.json", r#"{"scripts":{"b":"x"}}"#);

    let depth = scan(
        &fixture,
        "p",
        ScanConfig {
            max_depth: 1,
            ..ScanConfig::default()
        },
    );
    assert_eq!(depth.suggestions.len(), 2);
    assert_eq!(
        warning_count(&depth, DetectionWarningKind::DepthLimitReached),
        1
    );

    let directories = scan(
        &fixture,
        "p",
        ScanConfig {
            max_directories: 2,
            ..ScanConfig::default()
        },
    );
    assert_eq!(directories.scanned_directories, 2);
    assert_eq!(
        warning_count(&directories, DetectionWarningKind::DirectoryLimitReached),
        1
    );

    let packages = scan(
        &fixture,
        "p",
        ScanConfig {
            max_package_json_files: 1,
            ..ScanConfig::default()
        },
    );
    assert_eq!(packages.suggestions.len(), 1);
    assert_eq!(
        warning_count(&packages, DetectionWarningKind::PackageJsonLimitReached),
        1
    );
    assert!(depth.truncated && directories.truncated && packages.truncated);
}

#[test]
fn suggestion_and_one_mib_package_limits_are_enforced() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"a":"x","b":"x","c":"x"}}"#);
    let suggestions = scan(
        &fixture,
        "p",
        ScanConfig {
            max_suggestions: 2,
            ..ScanConfig::default()
        },
    );
    assert_eq!(suggestions.suggestions.len(), 2);
    assert_eq!(
        warning_count(&suggestions, DetectionWarningKind::SuggestionLimitReached),
        1
    );

    fixture.write_bytes("large/package.json", &vec![b' '; (1024 * 1024) + 1]);
    let large = scan(&fixture, "p", ScanConfig::default());
    assert_eq!(
        warning_count(&large, DetectionWarningKind::PackageJsonTooLarge),
        1
    );
    assert!(large.truncated);
}

#[test]
fn production_scan_limits_have_the_required_values() {
    assert_eq!(MAX_DEPTH, 5);
    assert_eq!(MAX_DIRECTORIES, 2_000);
    assert_eq!(MAX_PACKAGE_JSON_FILES, 200);
    assert_eq!(MAX_SUGGESTIONS, 500);
    assert_eq!(MAX_PACKAGE_JSON_BYTES, 1024 * 1024);
}

#[test]
#[cfg(windows)]
fn directory_and_package_symlinks_are_skipped_and_cannot_escape_root() {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let project = TempFixture::new();
    let outside = TempFixture::new();
    outside.write("package.json", r#"{"scripts":{"outside":"x"}}"#);
    if let Err(error) = symlink_dir(outside.path(""), project.path("linked")) {
        eprintln!("symlink test unavailable: {error}");
        return;
    }
    if let Err(error) = symlink_file(outside.path("package.json"), project.path("package.json")) {
        eprintln!("symlink test unavailable: {error}");
        return;
    }

    let result = scan(&project, "p", ScanConfig::default());
    assert!(result.suggestions.is_empty());
    assert_eq!(
        warning_count(&result, DetectionWarningKind::SymlinkOrJunctionSkipped),
        2
    );
}

#[test]
#[cfg(windows)]
fn windows_reparse_attribute_classifier_uses_the_native_flag() {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    assert!(has_reparse_attribute(FILE_ATTRIBUTE_REPARSE_POINT));
    assert!(has_reparse_attribute(FILE_ATTRIBUTE_REPARSE_POINT | 0x20));
    assert!(!has_reparse_attribute(0x20));
}

fn remove_gone_branch_before_read(path: &std::path::Path) -> io::Result<()> {
    if path.file_name().and_then(|name| name.to_str()) == Some("gone-branch") {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn deny_branch_before_read(path: &std::path::Path) -> io::Result<()> {
    if path.file_name().and_then(|name| name.to_str()) == Some("denied-branch") {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(())
}

fn fail_nested_packages_before_read(path: &std::path::Path) -> io::Result<()> {
    if path.file_name().and_then(|name| name.to_str()) != Some("package.json") {
        return Ok(());
    }
    match path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(|name| name.to_str())
    {
        Some("gone-package") => fs::remove_file(path),
        Some("denied-package") => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        _ => Ok(()),
    }
}

#[test]
fn disappearing_subdirectory_truncates_and_keeps_prior_suggestions() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"dev":"x"}}"#);
    fixture.write("gone-branch/package.json", r#"{"scripts":{"lost":"x"}}"#);

    let result = scan(
        &fixture,
        "p",
        ScanConfig {
            before_read: Some(remove_gone_branch_before_read),
            ..ScanConfig::default()
        },
    );

    assert!(result.truncated);
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(result.suggestions[0].command, "npm run -- dev");
    assert_eq!(
        warning_count(&result, DetectionWarningKind::FileDisappeared),
        1
    );
    assert_eq!(result.warnings[0].path.as_deref(), Some("gone-branch"));
}

#[test]
fn unreadable_subdirectory_truncates_and_keeps_prior_suggestions() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"dev":"x"}}"#);
    fixture.write("denied-branch/package.json", r#"{"scripts":{"lost":"x"}}"#);

    let result = scan(
        &fixture,
        "p",
        ScanConfig {
            before_read: Some(deny_branch_before_read),
            ..ScanConfig::default()
        },
    );

    assert!(result.truncated);
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(
        warning_count(&result, DetectionWarningKind::PermissionDenied),
        1
    );
    assert_eq!(result.warnings[0].path.as_deref(), Some("denied-branch"));
}

#[test]
fn unreadable_or_disappeared_package_truncates_without_duplicate_warnings() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"dev":"x"}}"#);
    fixture.write("denied-package/package.json", r#"{"scripts":{"lost":"x"}}"#);
    fixture.write("gone-package/package.json", r#"{"scripts":{"lost":"x"}}"#);

    let result = scan(
        &fixture,
        "p",
        ScanConfig {
            before_read: Some(fail_nested_packages_before_read),
            ..ScanConfig::default()
        },
    );

    assert!(result.truncated);
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(
        warning_count(&result, DetectionWarningKind::PermissionDenied),
        1
    );
    assert_eq!(
        warning_count(&result, DetectionWarningKind::FileDisappeared),
        1
    );
}

#[test]
fn node_scripts_follow_command_and_default_selection_policy() {
    let fixture = TempFixture::new();
    fixture.write(
        "package.json",
        r#"{"scripts":{"dev":"x","START":"x","serve":"x","Preview":"x","build":"x","test":"x","lint":"x","format":"x","typecheck":"x","clean":"x","prepare":"x","postinstall":"x","preinstall":"x","build client":"x","empty":"  ","number":7}}"#,
    );
    let result = scan(&fixture, "p", ScanConfig::default());

    assert_eq!(result.suggestions.len(), 14);
    assert_eq!(
        warning_count(&result, DetectionWarningKind::EmptyNpmScript),
        1
    );
    assert_eq!(
        suggestion(&result, "npm run -- dev").display_name,
        "npm: dev"
    );
    assert_eq!(suggestion(&result, "npm run -- dev").working_directory, ".");
    assert_eq!(
        suggestion(&result, "npm run -- dev").source_path,
        "package.json"
    );
    let spaced_command = format!(
        "npm run -- {}build client{}",
        char::from(34),
        char::from(34)
    );
    assert_eq!(suggestion(&result, &spaced_command).command, spaced_command);
    for command in [
        "npm run -- dev",
        "npm run -- START",
        "npm run -- serve",
        "npm run -- Preview",
    ] {
        assert!(suggestion(&result, command).default_selected);
    }
    for command in [
        "npm run -- build",
        "npm run -- test",
        "npm run -- lint",
        "npm run -- format",
        "npm run -- typecheck",
        "npm run -- clean",
        "npm run -- prepare",
        "npm run -- postinstall",
        "npm run -- preinstall",
    ] {
        assert!(!suggestion(&result, command).default_selected);
    }
}

#[test]
fn npm_option_separator_preserves_regular_spaced_and_dash_prefixed_names() {
    let fixture = TempFixture::new();
    fixture.write(
        "package.json",
        r#"{"scripts":{"dev":"x","build client":"x","-v":"x","--version":"x","-s":"x"}}"#,
    );

    let result = scan(&fixture, "p", ScanConfig::default());
    let spaced_command = format!(
        "npm run -- {}build client{}",
        char::from(34),
        char::from(34)
    );
    for command in [
        "npm run -- dev",
        spaced_command.as_str(),
        "npm run -- -v",
        "npm run -- --version",
        "npm run -- -s",
    ] {
        assert_eq!(suggestion(&result, command).command, command);
    }
    assert!(result.warnings.is_empty());
}

#[test]
fn unsafe_npm_names_are_rejected_without_creative_quoting() {
    let fixture = TempFixture::new();
    let mut scripts = serde_json::Map::new();
    for byte in [
        34_u8, 13, 10, 0, 38, 124, 60, 62, 94, 37, 33, 40, 41, 9, 92, 36,
    ] {
        let name = format!("bad{}name", char::from(byte));
        scripts.insert(name, serde_json::Value::String("x".to_owned()));
    }
    let package = serde_json::json!({ "scripts": scripts }).to_string();
    fixture.write("package.json", &package);

    let result = scan(&fixture, "p", ScanConfig::default());
    assert!(result.suggestions.is_empty());
    assert_eq!(
        warning_count(&result, DetectionWarningKind::UnsafeNpmScriptName),
        16
    );
}

#[test]
fn nested_packages_set_display_working_directory_and_source_path() {
    let fixture = TempFixture::new();
    fixture.write("client/package.json", r#"{"scripts":{"dev":"x"}}"#);
    fixture.write("server/package.json", r#"{"scripts":{"start":"x"}}"#);
    let result = scan(&fixture, "p", ScanConfig::default());

    let client = suggestion(&result, "npm run -- dev");
    assert_eq!(client.display_name, "client \u{2014} npm: dev");
    assert_eq!(client.working_directory, "client");
    assert_eq!(client.source_path, "client/package.json");
    let server = suggestion(&result, "npm run -- start");
    assert_eq!(server.display_name, "server \u{2014} npm: start");
    assert_eq!(server.working_directory, "server");
    assert_eq!(server.source_path, "server/package.json");
}

#[test]
fn invalid_json_and_invalid_shapes_warn_and_scanning_continues() {
    let fixture = TempFixture::new();
    fixture.write("a/package.json", "not json");
    fixture.write("b/package.json", r#"[]"#);
    fixture.write("c/package.json", r#"{"scripts":[]}"#);
    fixture.write("z/package.json", r#"{"scripts":{"dev":"x"}}"#);

    let result = scan(&fixture, "p", ScanConfig::default());
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(
        warning_count(&result, DetectionWarningKind::InvalidPackageJson),
        3
    );
    assert!(!result.truncated);
}

#[test]
fn stable_ids_use_project_source_path_and_exact_script_name_only() {
    let fixture = TempFixture::new();
    fixture.write(
        "package.json",
        r#"{"name":"one","scripts":{"dev":"value one","start":"x"}}"#,
    );
    fixture.write("client/package.json", r#"{"scripts":{"dev":"x"}}"#);
    let first = scan(&fixture, "project-a", ScanConfig::default());
    let root_dev = suggestion(&first, "npm run -- dev").stable_id.clone();
    let root_start = suggestion(&first, "npm run -- start").stable_id.clone();
    let client_dev = first
        .suggestions
        .iter()
        .find(|item| item.source_path == "client/package.json")
        .expect("client suggestion should exist")
        .stable_id
        .clone();

    fixture.write(
        "package.json",
        r#"{"name":"two","scripts":{"dev":"changed","start":"x"}}"#,
    );
    let changed_value = scan(&fixture, "project-a", ScanConfig::default());
    let other_project = scan(&fixture, "project-b", ScanConfig::default());

    assert_eq!(
        root_dev,
        suggestion(&changed_value, "npm run -- dev").stable_id
    );
    assert_ne!(root_dev, client_dev);
    assert_ne!(root_dev, root_start);
    assert_ne!(
        root_dev,
        suggestion(&other_project, "npm run -- dev").stable_id
    );
}

#[test]
fn security_fixture_never_executes_scripts_or_scans_node_modules() {
    let fixture = TempFixture::new();
    let marker = fixture.path("executed.marker");
    let marker_command = format!(
        "powershell -NoProfile -Command Set-Content -Path {} -Value executed",
        marker.display()
    );
    let package = serde_json::json!({
        "scripts": {
            "dev": marker_command,
            "start": "node -e process.exit(99)",
            "build": "cmd /c exit 99"
        }
    })
    .to_string();
    fixture.write("package.json", &package);
    fixture.write(
        "server/package.json",
        r#"{"scripts":{"serve":"npm --version"}}"#,
    );
    fixture.write(
        "node_modules/ignored/package.json",
        r#"{"scripts":{"hidden":"x"}}"#,
    );

    let result = scan(&fixture, "p", ScanConfig::default());
    assert_eq!(result.suggestions.len(), 4);
    assert!(result
        .suggestions
        .iter()
        .all(|item| !item.source_path.starts_with("node_modules/")));
    assert_path_absent(&marker);
}

#[test]
fn async_detection_keeps_projects_store_byte_identical_and_releases_its_lock() {
    fn assert_send<T: Send>(_: T) {}

    let fixture = TempFixture::new();
    let (state, project_id, data_file) = registered_state(&fixture);
    fixture.write(
        "project/package.json",
        r#"{"scripts":{"dev":"x","build":"x"}}"#,
    );
    let before = fs::read(&data_file).expect("projects data should be readable");

    assert_send(detect_registered_project(project_id.clone(), &state));
    let result =
        tauri::async_runtime::block_on(detect_registered_project(project_id.clone(), &state))
            .expect("registered project should scan");
    let after = fs::read(&data_file).expect("projects data should remain readable");

    assert_eq!(result.project_id, project_id);
    assert_eq!(result.suggestions.len(), 2);
    assert_eq!(before, after);
    assert!(state.store().is_ok());
}

#[test]
fn unknown_project_and_invalid_registered_root_are_hard_errors() {
    let fixture = TempFixture::new();
    let (state, _, _) = registered_state(&fixture);
    let error = tauri::async_runtime::block_on(detect_registered_project(
        "missing-project".to_owned(),
        &state,
    ))
    .expect_err("unknown project should fail");
    assert_eq!(error.code, "project_not_found");

    let missing_root = fixture.path("does-not-exist");
    let error = scan_project("p", &missing_root, ScanConfig::default())
        .expect_err("missing root should fail");
    assert_eq!(error.code, "directory_not_found");
}

#[test]
fn serialization_is_camel_case_and_keeps_stable_enum_values_and_warnings() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"dev":"x","empty":" "}}"#);
    let result = scan(&fixture, "project-a", ScanConfig::default());
    let json = serde_json::to_value(&result).expect("result should serialize");

    assert_eq!(json["projectId"], "project-a");
    assert!(json.get("projectRoot").is_some());
    assert!(json.get("scannedDirectories").is_some());
    assert!(json.get("project_id").is_none());
    assert_eq!(json["suggestions"][0]["sourceKind"], "npmScript");
    assert_eq!(json["suggestions"][0]["sourcePath"], "package.json");
    assert_eq!(json["suggestions"][0]["workingDirectory"], ".");
    assert_eq!(json["warnings"][0]["kind"], "emptyNpmScript");
    assert_eq!(json["warnings"][0]["path"], "package.json");
    assert!(json["warnings"][0]["message"].is_string());

    assert_eq!(
        serde_json::to_value(SourceKind::NpmScript).expect("kind should serialize"),
        "npmScript"
    );
    let warning = DetectionWarning {
        kind: DetectionWarningKind::FileDisappeared,
        message: "gone".to_owned(),
        path: Some("client/package.json".to_owned()),
    };
    assert_eq!(
        serde_json::to_value(warning).expect("warning should serialize"),
        serde_json::json!({
            "kind": "fileDisappeared",
            "message": "gone",
            "path": "client/package.json"
        })
    );
}
