use super::commands::{detect_registered_project, detect_registered_project_with_config};
use super::detection_test_support::{
    assert_path_absent, registered_state, scan, suggestion, warning_count, TempFixture,
};
use super::model::{DetectionWarning, DetectionWarningKind, SourceKind};
use super::scanner::{
    push_suggestion, scan_project, ScanConfig, MAX_DEPTH, MAX_DIRECTORIES, MAX_PACKAGE_JSON_BYTES,
    MAX_PACKAGE_JSON_FILES, MAX_SUGGESTIONS, MAX_WINDOWS_SCRIPTS,
};
use super::windows_scripts::inspect_windows_script;
use crate::projects::ServiceInput;
use std::collections::HashSet;
use std::fs::{self, FileTimes, OpenOptions};
use std::io;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

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
    assert!(first
        .suggestions
        .iter()
        .filter(|item| item.source_kind != SourceKind::NpmScript)
        .all(|item| !item.default_selected && item.editable));
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
    assert_eq!(MAX_WINDOWS_SCRIPTS, 200);
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

fn remove_windows_script_before_second_metadata(path: &std::path::Path) -> io::Result<()> {
    if path.file_name().and_then(|name| name.to_str()) == Some("z-gone.cmd") {
        fs::remove_file(path)?;
    }
    Ok(())
}

type ScanGate = (mpsc::Sender<()>, mpsc::Receiver<()>);
static LOCK_SCAN_GATE: OnceLock<Mutex<Option<ScanGate>>> = OnceLock::new();

fn block_first_scan_read(path: &std::path::Path) -> io::Result<()> {
    let _ = path;
    let channels = LOCK_SCAN_GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| io::Error::other("scan gate poisoned"))?
        .take();
    if let Some((entered, release)) = channels {
        entered
            .send(())
            .map_err(|_| io::Error::other("scan gate receiver disappeared"))?;
        release
            .recv()
            .map_err(|_| io::Error::other("scan gate sender disappeared"))?;
    }
    Ok(())
}

#[test]
fn windows_scripts_use_exact_commands_fields_extensions_and_deterministic_order() {
    let fixture = TempFixture::new();
    fixture.write("Alpha.CMD", "ignored");
    fixture.write("package.json", r#"{"scripts":{"dev":"ignored"}}"#);
    fixture.write("run.BAT", "ignored");
    fixture.write("setup.PS1", "ignored");
    fixture.write("tools/My deploy.Ps1", "ignored");
    fixture.write("tools/déployer-1_test.cmd", "ignored");
    let first = scan(&fixture, "project-a", ScanConfig::default());
    let second = scan(&fixture, "project-a", ScanConfig::default());
    assert_eq!(first, second);
    assert_eq!(
        first
            .suggestions
            .iter()
            .map(|item| item.source_path.as_str())
            .collect::<Vec<_>>(),
        [
            "Alpha.CMD",
            "package.json",
            "run.BAT",
            "setup.PS1",
            "tools/déployer-1_test.cmd",
            "tools/My deploy.Ps1"
        ]
    );
    let cmd_command = format!("{}.\\Alpha.CMD{}", char::from(34), char::from(34));
    let cmd = suggestion(&first, &cmd_command);
    assert_eq!(cmd.source_kind, SourceKind::Cmd);
    assert_eq!(cmd.working_directory, ".");
    assert_eq!(cmd.display_name, "CMD: Alpha.CMD");
    assert!(!cmd.default_selected);
    assert!(cmd.editable);

    let bat_command = format!("{}.\\run.BAT{}", char::from(34), char::from(34));
    let bat = suggestion(&first, &bat_command);
    assert_eq!(bat.source_kind, SourceKind::Bat);
    assert_eq!(bat.display_name, "BAT: run.BAT");

    let powershell_command = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -File {}.\\setup.PS1{}",
        char::from(34),
        char::from(34)
    );
    let powershell = suggestion(&first, &powershell_command);
    assert_eq!(powershell.source_kind, SourceKind::PowerShell);
    assert_eq!(powershell.display_name, "PowerShell: setup.PS1");
    assert!(powershell.reason.contains("Windows PowerShell script"));

    let nested_command = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -File {}.\\My deploy.Ps1{}",
        char::from(34),
        char::from(34)
    );
    let nested = suggestion(&first, &nested_command);
    assert_eq!(nested.working_directory, "tools");
    assert_eq!(nested.source_path, "tools/My deploy.Ps1");
    assert_eq!(
        nested.display_name,
        format!(
            "tools {} PowerShell: My deploy.Ps1",
            char::from_u32(8212).expect("em dash should be valid")
        )
    );
    let unicode_command = format!("{}.\\déployer-1_test.cmd{}", char::from(34), char::from(34));
    assert_eq!(
        suggestion(&first, &unicode_command).source_kind,
        SourceKind::Cmd
    );
}

#[test]
fn windows_script_contents_are_not_read_or_executed_and_exclusions_apply() {
    let fixture = TempFixture::new();
    let ps_marker = fixture.path("powershell.marker");
    let cmd_marker = fixture.path("cmd.marker");
    let bat_marker = fixture.path("bat.marker");
    fixture.write_bytes("opaque.ps1", &[0, 0xff, 0xfe, 0xfd]);
    fixture.write(
        "marker.ps1",
        &format!(
            "Set-Content -LiteralPath '{}' -Value executed",
            ps_marker.display()
        ),
    );
    fixture.write(
        "marker.cmd",
        &format!("@echo executed>{}", cmd_marker.display()),
    );
    fixture.write(
        "marker.bat",
        &format!("@echo executed>{}", bat_marker.display()),
    );
    for directory in ["node_modules", "dist", "target"] {
        fixture.write(&format!("{directory}/ignored.ps1"), "ignored");
        fixture.write(&format!("{directory}/ignored.cmd"), "ignored");
        fixture.write(&format!("{directory}/ignored.bat"), "ignored");
    }

    let result = scan(&fixture, "p", ScanConfig::default());
    assert_eq!(result.suggestions.len(), 4);
    assert!(result
        .suggestions
        .iter()
        .all(|item| !item.source_path.contains('/')));
    assert!(result
        .suggestions
        .iter()
        .any(|item| item.source_path == "opaque.ps1"));
    assert_path_absent(&ps_marker);
    assert_path_absent(&cmd_marker);
    assert_path_absent(&bat_marker);
}

#[test]
#[cfg(windows)]
fn locked_windows_script_proves_scanner_does_not_read_content() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::LockFile;

    let fixture = TempFixture::new();
    let script_path = fixture.write_bytes("locked.cmd", &[0xff; 32]);
    let locked_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&script_path)
        .expect("test script should open");
    let locked = unsafe { LockFile(locked_file.as_raw_handle() as _, 0, 0, u32::MAX, u32::MAX) };
    assert_ne!(locked, 0, "test should lock script contents");
    assert!(fs::read(&script_path).is_err());

    let result = scan(&fixture, "p", ScanConfig::default());
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(result.suggestions[0].source_path, "locked.cmd");
    drop(locked_file);
}

#[test]
fn windows_and_global_suggestion_limits_are_independent() {
    let fixture = TempFixture::new();
    fixture.write("a.cmd", "ignored");
    fixture.write("b.cmd", "ignored");
    fixture.write("c.cmd", "ignored");
    fixture.write("package.json", r#"{"scripts":{"dev":"ignored"}}"#);

    let windows_limited = scan(
        &fixture,
        "p",
        ScanConfig {
            max_windows_scripts: 1,
            ..ScanConfig::default()
        },
    );
    assert_eq!(
        windows_limited
            .suggestions
            .iter()
            .map(|item| item.source_path.as_str())
            .collect::<Vec<_>>(),
        ["a.cmd", "package.json"]
    );
    assert_eq!(
        warning_count(
            &windows_limited,
            DetectionWarningKind::WindowsScriptLimitReached
        ),
        1
    );
    assert!(windows_limited.truncated);

    let globally_limited = scan(
        &fixture,
        "p",
        ScanConfig {
            max_suggestions: 1,
            ..ScanConfig::default()
        },
    );
    assert_eq!(globally_limited.suggestions.len(), 1);
    assert_eq!(
        warning_count(
            &globally_limited,
            DetectionWarningKind::SuggestionLimitReached
        ),
        1
    );
    assert_eq!(
        warning_count(
            &globally_limited,
            DetectionWarningKind::WindowsScriptLimitReached
        ),
        0
    );
    assert!(globally_limited.truncated);
}

#[test]
fn windows_script_name_policy_rejects_shell_metacharacters() {
    for unsafe_character in [
        '"', '\r', '\n', '\0', '&', '|', '<', '>', '^', '%', '!', '(', ')', '\t', '\\', '$',
    ] {
        let file_name = format!("bad{unsafe_character}name.cmd");
        let inspection = inspect_windows_script("p", SourceKind::Cmd, &file_name, ".", &file_name);
        assert!(
            inspection.suggestion.is_none(),
            "accepted {unsafe_character:?}"
        );
        assert_eq!(inspection.warnings.len(), 1);
        assert_eq!(
            inspection.warnings[0].kind,
            DetectionWarningKind::UnsafeWindowsScriptName
        );
        assert_eq!(
            inspection.warnings[0].path.as_deref(),
            Some(file_name.as_str())
        );
    }

    let fixture = TempFixture::new();
    fixture.write("bad&name.cmd", "ignored");
    let result = scan(&fixture, "p", ScanConfig::default());
    assert!(result.suggestions.is_empty());
    assert!(!result.truncated);
    assert_eq!(
        warning_count(&result, DetectionWarningKind::UnsafeWindowsScriptName),
        1
    );
    assert_eq!(result.warnings[0].path.as_deref(), Some("bad&name.cmd"));
}

#[test]
#[cfg(windows)]
fn windows_script_symlink_is_skipped_without_reading_its_target() {
    use std::os::windows::fs::symlink_file;

    let project = TempFixture::new();
    let outside = TempFixture::new();
    outside.write("outside.cmd", "ignored");
    if let Err(error) = symlink_file(outside.path("outside.cmd"), project.path("linked.cmd")) {
        eprintln!("symlink test unavailable: {error}");
        return;
    }

    let result = scan(&project, "p", ScanConfig::default());
    assert!(result.suggestions.is_empty());
    assert!(!result.truncated);
    assert_eq!(
        warning_count(&result, DetectionWarningKind::SymlinkOrJunctionSkipped),
        1
    );
}

#[test]
fn disappearing_identified_windows_script_truncates_and_keeps_prior_suggestions() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"dev":"ignored"}}"#);
    fixture.write("z-gone.cmd", "ignored");

    let result = scan(
        &fixture,
        "p",
        ScanConfig {
            before_read: Some(remove_windows_script_before_second_metadata),
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
    assert_eq!(result.warnings[0].path.as_deref(), Some("z-gone.cmd"));
}

#[test]
fn windows_stable_ids_cover_project_path_kind_and_ignore_file_metadata() {
    let fixture = TempFixture::new();
    fixture.write("same.ps1", "first content");
    fixture.write("same.cmd", "first content");
    fixture.write("same.bat", "first content");
    fixture.write("nested/same.ps1", "first content");

    let first = scan(&fixture, "project-a", ScanConfig::default());
    let second = scan(&fixture, "project-a", ScanConfig::default());
    let other_project = scan(&fixture, "project-b", ScanConfig::default());
    let id = |result: &super::model::DetectionResult, path: &str| {
        result
            .suggestions
            .iter()
            .find(|item| item.source_path == path)
            .expect("Windows suggestion should exist")
            .stable_id
            .clone()
    };

    let root_ps = id(&first, "same.ps1");
    assert_eq!(root_ps, id(&second, "same.ps1"));
    assert_ne!(root_ps, id(&first, "nested/same.ps1"));
    assert_ne!(root_ps, id(&first, "same.cmd"));
    assert_ne!(root_ps, id(&first, "same.bat"));
    assert_ne!(root_ps, id(&other_project, "same.ps1"));

    let script_path = fixture.write("same.ps1", "changed content");
    OpenOptions::new()
        .write(true)
        .open(&script_path)
        .expect("script should open for the test only")
        .set_times(
            FileTimes::new()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        )
        .expect("test should set the timestamp");
    let changed = scan(&fixture, "project-a", ScanConfig::default());
    assert_eq!(root_ps, id(&changed, "same.ps1"));
}

#[test]
fn duplicate_stable_ids_keep_the_first_suggestion_without_truncating() {
    let fixture = TempFixture::new();
    fixture.write("package.json", r#"{"scripts":{"dev":"ignored"}}"#);
    let mut result = scan(&fixture, "p", ScanConfig::default());
    let first = result.suggestions.remove(0);
    let mut duplicate = first.clone();
    duplicate.display_name = "duplicate should be discarded".to_owned();
    result.warnings.clear();
    result.truncated = false;
    let mut stable_ids = HashSet::new();

    assert!(push_suggestion(
        &mut result,
        &mut stable_ids,
        first.clone(),
        MAX_SUGGESTIONS,
        None,
    ));
    assert!(push_suggestion(
        &mut result,
        &mut stable_ids,
        duplicate,
        MAX_SUGGESTIONS,
        None,
    ));
    assert_eq!(result.suggestions, [first]);
    assert!(!result.truncated);
}

#[test]
#[cfg(windows)]
fn registered_services_match_all_source_kinds_without_mutating_the_store() {
    let fixture = TempFixture::new();
    let (state, project_id, data_file) = registered_state(&fixture);
    let npm_marker = fixture.path("npm.marker");
    let ps_marker = fixture.path("powershell.marker");
    let cmd_marker = fixture.path("cmd.marker");
    let bat_marker = fixture.path("bat.marker");
    fixture.write(
        "project/package.json",
        &serde_json::json!({
            "scripts": {
                "dev": format!("echo executed>{}", npm_marker.display()),
                "build": "ignored"
            }
        })
        .to_string(),
    );
    fixture.write(
        "project/launch.ps1",
        &format!(
            "Set-Content -LiteralPath '{}' -Value executed",
            ps_marker.display()
        ),
    );
    fixture.write("project/fresh.ps1", "ignored");
    fixture.write(
        "project/tools/deploy.CMD",
        &format!("@echo executed>{}", cmd_marker.display()),
    );
    fixture.write(
        "project/run.bat",
        &format!("@echo executed>{}", bat_marker.display()),
    );
    fixture.write("project/node_modules/ignored.cmd", "ignored");

    let ps_command = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -File {}.\\launch.ps1{}",
        char::from(34),
        char::from(34)
    );
    let cmd_command = format!("{}.\\deploy.CMD{}", char::from(34), char::from(34));
    let bat_command = format!("{}.\\run.bat{}", char::from(34), char::from(34));
    let add_service = |name: &str, working_directory: &str, command: &str| {
        state
            .store()
            .expect("store lock should be available")
            .add_service(
                &project_id,
                &ServiceInput {
                    name: name.to_owned(),
                    working_directory: working_directory.to_owned(),
                    command: command.to_owned(),
                    expected_port: None,
                    local_url: None,
                },
            )
            .expect("service should register");
    };
    add_service("npm dev", "./", " npm run -- dev ");
    add_service("duplicate npm dev", ".", "npm run -- dev");
    add_service("PowerShell", ".", &ps_command);
    add_service("CMD", "TOOLS\\.", &cmd_command);
    add_service("BAT", ".", &bat_command);

    let before = fs::read(&data_file).expect("store should be readable");
    let result =
        tauri::async_runtime::block_on(detect_registered_project(project_id.clone(), &state))
            .expect("registered project should scan");
    let after = fs::read(&data_file).expect("store should remain readable");

    assert_eq!(result.suggestions.len(), 6);
    assert_eq!(before, after);
    let matches_registered = |item: &super::model::ServiceSuggestion| {
        item.warnings
            .iter()
            .filter(|warning| warning.kind == DetectionWarningKind::MatchesRegisteredService)
            .count()
    };
    let npm = suggestion(&result, "npm run -- dev");
    assert!(!npm.default_selected);
    assert_eq!(matches_registered(npm), 1);
    assert_eq!(matches_registered(suggestion(&result, &ps_command)), 1);
    assert_eq!(matches_registered(suggestion(&result, &cmd_command)), 1);
    assert_eq!(matches_registered(suggestion(&result, &bat_command)), 1);

    let fresh = result
        .suggestions
        .iter()
        .find(|item| item.source_path == "fresh.ps1")
        .expect("unmatched PowerShell suggestion should exist");
    assert_eq!(matches_registered(fresh), 0);
    assert_eq!(
        matches_registered(suggestion(&result, "npm run -- build")),
        0
    );
    assert_eq!(
        warning_count(&result, DetectionWarningKind::MatchesRegisteredService),
        0
    );
    assert!(result
        .suggestions
        .iter()
        .all(|item| !item.source_path.starts_with("node_modules/")));
    assert_path_absent(&npm_marker);
    assert_path_absent(&ps_marker);
    assert_path_absent(&cmd_marker);
    assert_path_absent(&bat_marker);
}

#[test]
fn detection_releases_the_store_lock_before_scanning() {
    let fixture = TempFixture::new();
    let (state, project_id, _) = registered_state(&fixture);
    fixture.write("project/package.json", r#"{"scripts":{"dev":"ignored"}}"#);
    let state = Arc::new(state);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *LOCK_SCAN_GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("scan gate should lock") = Some((entered_tx, release_rx));

    let detection_state = Arc::clone(&state);
    let detection = std::thread::spawn(move || {
        tauri::async_runtime::block_on(detect_registered_project_with_config(
            project_id,
            &detection_state,
            ScanConfig {
                before_read: Some(block_first_scan_read),
                ..ScanConfig::default()
            },
        ))
    });
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("scanner should enter its blocking hook");

    let lock_state = Arc::clone(&state);
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let lock_attempt = std::thread::spawn(move || {
        let acquired = lock_state.store().is_ok();
        let _ = acquired_tx.send(acquired);
    });
    let acquired_before_release = acquired_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or(false);
    release_tx
        .send(())
        .expect("blocked scanner should still be waiting");

    let scan_result = detection.join().expect("detection thread should join");
    lock_attempt.join().expect("lock thread should join");
    assert!(acquired_before_release);
    assert_eq!(
        scan_result
            .expect("detection should succeed")
            .suggestions
            .len(),
        1
    );
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
    assert_eq!(
        serde_json::to_value(SourceKind::PowerShell).expect("kind should serialize"),
        "powerShell"
    );
    assert_eq!(
        serde_json::to_value(SourceKind::Cmd).expect("kind should serialize"),
        "cmd"
    );
    assert_eq!(
        serde_json::to_value(SourceKind::Bat).expect("kind should serialize"),
        "bat"
    );
    assert_eq!(
        serde_json::to_value(DetectionWarningKind::UnsafeWindowsScriptName)
            .expect("warning kind should serialize"),
        "unsafeWindowsScriptName"
    );
    assert_eq!(
        serde_json::to_value(DetectionWarningKind::MatchesRegisteredService)
            .expect("warning kind should serialize"),
        "matchesRegisteredService"
    );
    assert_eq!(
        serde_json::to_value(DetectionWarningKind::WindowsScriptLimitReached)
            .expect("warning kind should serialize"),
        "windowsScriptLimitReached"
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
