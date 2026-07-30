use super::error::DetectionError;
use super::model::{DetectionResult, DetectionWarning, DetectionWarningKind};
use super::node::{inspect_package, PackageContext};
use crate::projects::{is_path_within, normalize_existing_directory};
use std::fs::{self, DirEntry};
use std::io;
use std::path::Path;

// Root is depth 0; depth 5 directories are inspected, but depth 6 is not entered.
pub(super) const MAX_DEPTH: u32 = 5;
pub(super) const MAX_DIRECTORIES: u32 = 2_000;
pub(super) const MAX_PACKAGE_JSON_FILES: u32 = 200;
pub(super) const MAX_SUGGESTIONS: usize = 500;
pub(super) const MAX_PACKAGE_JSON_BYTES: u64 = 1024 * 1024;

const EXCLUDED_DIRECTORIES: &[&str] = &[
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
];

#[derive(Clone, Copy)]
pub(super) struct ScanConfig {
    pub max_depth: u32,
    pub max_directories: u32,
    pub max_package_json_files: u32,
    pub max_suggestions: usize,
    pub max_package_json_bytes: u64,
    pub before_read: Option<fn(&Path) -> io::Result<()>>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            max_depth: MAX_DEPTH,
            max_directories: MAX_DIRECTORIES,
            max_package_json_files: MAX_PACKAGE_JSON_FILES,
            max_suggestions: MAX_SUGGESTIONS,
            max_package_json_bytes: MAX_PACKAGE_JSON_BYTES,
            before_read: None,
        }
    }
}

pub(super) fn scan_project(
    project_id: &str,
    registered_root: &Path,
    config: ScanConfig,
) -> Result<DetectionResult, DetectionError> {
    let root = normalize_existing_directory(registered_root.to_string_lossy().as_ref())?;
    let mut result = DetectionResult {
        project_id: project_id.to_owned(),
        project_root: root.to_string_lossy().into_owned(),
        suggestions: Vec::new(),
        warnings: Vec::new(),
        scanned_directories: 0,
        truncated: false,
    };
    let mut pending = vec![(root.clone(), 0_u32)];
    let mut package_count = 0_u32;

    'scan: while let Some((directory, depth)) = pending.pop() {
        if result.scanned_directories >= config.max_directories {
            add_limit_warning(
                &mut result,
                DetectionWarningKind::DirectoryLimitReached,
                "The directory scan limit was reached.",
                relative_path(&root, &directory),
            );
            break;
        }
        result.scanned_directories += 1;

        let entries = match before_read(config, &directory)
            .and_then(|_| sorted_entries(&root, &directory, &mut result))
        {
            Ok(entries) => entries,
            Err(error) if depth == 0 => {
                return Err(DetectionError::inaccessible_root(
                    &directory,
                    error.to_string(),
                ));
            }
            Err(error) => {
                result.truncated = true;
                result.warnings.push(filesystem_warning(
                    &error,
                    "read directory",
                    relative_path(&root, &directory),
                ));
                continue;
            }
        };
        let mut child_directories = Vec::new();

        for entry in entries {
            let entry_path = entry.path();
            let entry_name = entry.file_name();
            let Some(entry_name) = entry_name.to_str() else {
                result.warnings.push(DetectionWarning {
                    kind: DetectionWarningKind::NonUtf8Path,
                    message: "A filesystem entry with a non-UTF-8 name was skipped.".to_owned(),
                    path: relative_path(&root, &directory),
                });
                continue;
            };
            if !is_path_within(&root, &entry_path) {
                continue;
            }

            let metadata = match fs::symlink_metadata(&entry_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    result.truncated = true;
                    result.warnings.push(filesystem_warning(
                        &error,
                        "inspect filesystem entry",
                        relative_path(&root, &entry_path),
                    ));
                    continue;
                }
            };
            if is_link_or_reparse_point(&metadata) {
                result.warnings.push(DetectionWarning {
                    kind: DetectionWarningKind::SymlinkOrJunctionSkipped,
                    message: "A symbolic link, junction, or reparse point was skipped.".to_owned(),
                    path: relative_path(&root, &entry_path),
                });
                continue;
            }

            if metadata.is_dir() {
                if is_excluded_directory(entry_name) {
                    continue;
                }
                if depth >= config.max_depth {
                    add_limit_warning(
                        &mut result,
                        DetectionWarningKind::DepthLimitReached,
                        "The directory depth limit was reached.",
                        relative_path(&root, &entry_path),
                    );
                } else {
                    child_directories.push(entry_path);
                }
                continue;
            }
            if entry_name != "package.json" || !metadata.is_file() {
                continue;
            }
            if package_count >= config.max_package_json_files {
                add_limit_warning(
                    &mut result,
                    DetectionWarningKind::PackageJsonLimitReached,
                    "The package.json scan limit was reached.",
                    relative_path(&root, &entry_path),
                );
                break 'scan;
            }
            package_count += 1;
            let source_path =
                relative_path(&root, &entry_path).unwrap_or_else(|| "package.json".to_owned());
            if metadata.len() > config.max_package_json_bytes {
                result.truncated = true;
                result.warnings.push(DetectionWarning {
                    kind: DetectionWarningKind::PackageJsonTooLarge,
                    message: format!(
                        "package.json exceeds the {}-byte size limit.",
                        config.max_package_json_bytes
                    ),
                    path: Some(source_path),
                });
                continue;
            }
            let working_directory =
                relative_path(&root, &directory).unwrap_or_else(|| ".".to_owned());
            if let Err(error) = before_read(config, &entry_path) {
                result.truncated = true;
                result.warnings.push(filesystem_warning(
                    &error,
                    "read package.json",
                    Some(source_path),
                ));
                continue;
            }
            let inspection = inspect_package(PackageContext {
                project_id,
                package_path: &entry_path,
                source_path: &source_path,
                working_directory: &working_directory,
                max_bytes: config.max_package_json_bytes,
            });
            if inspection.warnings.iter().any(|warning| {
                matches!(
                    warning.kind,
                    DetectionWarningKind::PermissionDenied
                        | DetectionWarningKind::FileDisappeared
                        | DetectionWarningKind::PackageJsonTooLarge
                )
            }) {
                result.truncated = true;
            }
            result.warnings.extend(inspection.warnings);
            let remaining = config
                .max_suggestions
                .saturating_sub(result.suggestions.len());
            if inspection.suggestions.len() > remaining {
                result
                    .suggestions
                    .extend(inspection.suggestions.into_iter().take(remaining));
                add_limit_warning(
                    &mut result,
                    DetectionWarningKind::SuggestionLimitReached,
                    "The service suggestion limit was reached.",
                    Some(source_path),
                );
                break 'scan;
            }
            result.suggestions.extend(inspection.suggestions);
        }

        child_directories.sort_by(|left, right| compare_names(left, right));
        pending.extend(
            child_directories
                .into_iter()
                .rev()
                .map(|path| (path, depth + 1)),
        );
    }
    Ok(result)
}

fn sorted_entries(
    root: &Path,
    directory: &Path,
    result: &mut DetectionResult,
) -> io::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                result.truncated = true;
                result.warnings.push(filesystem_warning(
                    &error,
                    "enumerate directory entry",
                    relative_path(root, directory),
                ));
            }
        }
    }
    entries.sort_by(|left, right| {
        compare_os_names(
            &left.file_name().to_string_lossy(),
            &right.file_name().to_string_lossy(),
        )
    });
    Ok(entries)
}

fn before_read(config: ScanConfig, path: &Path) -> io::Result<()> {
    config.before_read.map_or(Ok(()), |hook| hook(path))
}

fn compare_names(left: &Path, right: &Path) -> std::cmp::Ordering {
    compare_os_names(
        &left.file_name().unwrap_or_default().to_string_lossy(),
        &right.file_name().unwrap_or_default().to_string_lossy(),
    )
}

fn compare_os_names(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

fn is_excluded_directory(name: &str) -> bool {
    EXCLUDED_DIRECTORIES
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return Some(".".to_owned());
    }
    let mut segments = Vec::new();
    for component in relative.components() {
        segments.push(component.as_os_str().to_str()?);
    }
    Some(segments.join("/"))
}

fn add_limit_warning(
    result: &mut DetectionResult,
    kind: DetectionWarningKind,
    message: &str,
    path: Option<String>,
) {
    result.truncated = true;
    if result.warnings.iter().all(|warning| warning.kind != kind) {
        result.warnings.push(DetectionWarning {
            kind,
            message: message.to_owned(),
            path,
        });
    }
}

pub(super) fn filesystem_warning(
    error: &io::Error,
    action: &str,
    path: Option<String>,
) -> DetectionWarning {
    DetectionWarning {
        kind: if error.kind() == io::ErrorKind::NotFound {
            DetectionWarningKind::FileDisappeared
        } else {
            DetectionWarningKind::PermissionDenied
        },
        message: format!("Could not {action}: {error}"),
        path,
    }
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        has_reparse_attribute(metadata.file_attributes())
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
pub(super) fn has_reparse_attribute(attributes: u32) -> bool {
    attributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT != 0
}
