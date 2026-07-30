use crate::projects::error::ProjectError;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub(crate) fn normalize_existing_directory(raw_path: &str) -> Result<PathBuf, ProjectError> {
    let trimmed_path = raw_path.trim();
    if trimmed_path.is_empty() {
        return Err(ProjectError::InvalidPath {
            reason: "The path cannot be empty.".to_owned(),
        });
    }

    let path = PathBuf::from(trimmed_path);
    if !path.is_absolute() {
        return Err(ProjectError::InvalidPath {
            reason: "The path must be absolute.".to_owned(),
        });
    }

    let metadata = fs::metadata(&path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ProjectError::DirectoryNotFound { path: path.clone() },
        _ => ProjectError::PathUnavailable {
            path: path.clone(),
            reason: error.to_string(),
        },
    })?;

    if !metadata.is_dir() {
        return Err(ProjectError::NotDirectory { path });
    }

    let canonical_path =
        fs::canonicalize(&path).map_err(|error| ProjectError::PathUnavailable {
            path: path.clone(),
            reason: error.to_string(),
        })?;

    Ok(remove_windows_verbatim_prefix(canonical_path))
}

pub(crate) fn normalize_relative_directory_syntax(raw_path: &str) -> Result<String, ProjectError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidWorkingDirectory {
            reason: "The path cannot be empty. Use '.' for the project root.".to_owned(),
        });
    }
    if trimmed.contains('\0') {
        return Err(ProjectError::InvalidWorkingDirectory {
            reason: "NUL characters are not allowed.".to_owned(),
        });
    }

    let slash_path = trimmed.replace('\\', "/");
    if is_absolute_like(&slash_path) {
        return Err(ProjectError::InvalidWorkingDirectory {
            reason: "Absolute and drive-qualified paths are not allowed.".to_owned(),
        });
    }

    let mut segments = Vec::new();
    for segment in slash_path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                return Err(ProjectError::InvalidWorkingDirectory {
                    reason: "Parent components ('..') are not allowed.".to_owned(),
                });
            }
            _ => segments.push(segment),
        }
    }

    if segments.is_empty() {
        Ok(".".to_owned())
    } else {
        Ok(segments.join("/"))
    }
}

pub(crate) fn normalize_service_directory(
    project_root: &Path,
    raw_path: &str,
) -> Result<String, ProjectError> {
    let normalized = normalize_relative_directory_syntax(raw_path)?;
    resolve_normalized_service_directory(project_root, &normalized)?;
    Ok(normalized)
}

pub(crate) fn canonical_service_directory(
    project_root: &Path,
    raw_path: &str,
) -> Result<PathBuf, ProjectError> {
    let normalized = normalize_relative_directory_syntax(raw_path)?;
    resolve_normalized_service_directory(project_root, &normalized)
}

fn resolve_normalized_service_directory(
    project_root: &Path,
    normalized: &str,
) -> Result<PathBuf, ProjectError> {
    let canonical_root = normalize_existing_directory(project_root.to_string_lossy().as_ref())?;
    let candidate = if normalized == "." {
        canonical_root.clone()
    } else {
        canonical_root.join(Path::new(normalized))
    };

    let metadata = fs::metadata(&candidate).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ProjectError::WorkingDirectoryNotFound {
            path: candidate.clone(),
        },
        _ => ProjectError::PathUnavailable {
            path: candidate.clone(),
            reason: error.to_string(),
        },
    })?;
    if !metadata.is_dir() {
        return Err(ProjectError::WorkingDirectoryNotDirectory { path: candidate });
    }

    let canonical_candidate = fs::canonicalize(&candidate)
        .map(remove_windows_verbatim_prefix)
        .map_err(|error| ProjectError::PathUnavailable {
            path: candidate.clone(),
            reason: error.to_string(),
        })?;

    if !is_path_within(&canonical_root, &canonical_candidate) {
        return Err(ProjectError::WorkingDirectoryEscapesProject { path: candidate });
    }

    Ok(canonical_candidate)
}

pub(crate) fn relative_service_directory(
    project_root: &Path,
    selected_path: &str,
) -> Result<String, ProjectError> {
    let trimmed = selected_path.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidWorkingDirectory {
            reason: "The selected path cannot be empty.".to_owned(),
        });
    }

    let selected = PathBuf::from(trimmed);
    if !selected.is_absolute() {
        return Err(ProjectError::InvalidWorkingDirectory {
            reason: "The folder picker must return an absolute path.".to_owned(),
        });
    }

    let metadata = fs::metadata(&selected).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ProjectError::WorkingDirectoryNotFound {
            path: selected.clone(),
        },
        _ => ProjectError::PathUnavailable {
            path: selected.clone(),
            reason: error.to_string(),
        },
    })?;
    if !metadata.is_dir() {
        return Err(ProjectError::WorkingDirectoryNotDirectory { path: selected });
    }

    let canonical_root = normalize_existing_directory(project_root.to_string_lossy().as_ref())?;
    let canonical_selected = fs::canonicalize(&selected)
        .map(remove_windows_verbatim_prefix)
        .map_err(|error| ProjectError::PathUnavailable {
            path: selected.clone(),
            reason: error.to_string(),
        })?;

    if !is_path_within(&canonical_root, &canonical_selected) {
        return if is_path_within(&canonical_root, &selected) {
            Err(ProjectError::WorkingDirectoryEscapesProject { path: selected })
        } else {
            Err(ProjectError::WorkingDirectoryOutsideProject { path: selected })
        };
    }

    relative_from_root(&canonical_root, &canonical_selected)
}

pub(crate) fn path_key(path: &Path) -> String {
    let path_text = path.to_string_lossy();

    #[cfg(windows)]
    {
        let mut normalized = remove_windows_verbatim_prefix_text(&path_text).replace('/', "\\");
        while normalized.ends_with('\\') && !is_windows_root(&normalized) {
            normalized.pop();
        }
        normalized.to_lowercase()
    }

    #[cfg(not(windows))]
    {
        path_text.trim_end_matches('/').to_owned()
    }
}

fn is_absolute_like(path: &str) -> bool {
    if path.starts_with('/') {
        return true;
    }

    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[1] == b':') || Path::new(path).is_absolute()
}

pub(crate) fn is_path_within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        let root_key = path_key(root);
        let candidate_key = path_key(candidate);
        candidate_key == root_key
            || if root_key.ends_with('\\') {
                candidate_key.starts_with(&root_key)
            } else {
                candidate_key
                    .strip_prefix(&root_key)
                    .is_some_and(|suffix| suffix.starts_with('\\'))
            }
    }

    #[cfg(not(windows))]
    {
        candidate.starts_with(root)
    }
}

fn relative_from_root(root: &Path, candidate: &Path) -> Result<String, ProjectError> {
    let root_components: Vec<Component<'_>> = root.components().collect();
    let candidate_components: Vec<Component<'_>> = candidate.components().collect();

    if candidate_components.len() < root_components.len()
        || !root_components
            .iter()
            .zip(candidate_components.iter())
            .all(|(left, right)| components_equal(left, right))
    {
        return Err(ProjectError::WorkingDirectoryOutsideProject {
            path: candidate.to_path_buf(),
        });
    }

    let segments: Vec<String> = candidate_components[root_components.len()..]
        .iter()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    if segments.is_empty() {
        Ok(".".to_owned())
    } else {
        Ok(segments.join("/"))
    }
}

fn components_equal(left: &Component<'_>, right: &Component<'_>) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    }

    #[cfg(not(windows))]
    {
        left == right
    }
}

fn remove_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let path_text = path.to_string_lossy();
        PathBuf::from(remove_windows_verbatim_prefix_text(&path_text))
    }

    #[cfg(not(windows))]
    {
        path
    }
}

#[cfg(windows)]
fn remove_windows_verbatim_prefix_text(path: &str) -> String {
    if let Some(network_path) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network_path}")
    } else if let Some(local_path) = path.strip_prefix(r"\\?\") {
        local_path.to_owned()
    } else {
        path.to_owned()
    }
}

#[cfg(windows)]
fn is_windows_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\') || path == r"\"
}

#[cfg(test)]
mod tests {
    use super::{normalize_relative_directory_syntax, path_key};
    use crate::projects::error::ProjectError;
    use std::path::Path;

    #[test]
    fn relative_directory_normalizes_separators_and_redundant_components() {
        assert_eq!(
            normalize_relative_directory_syntax(r".\apps//frontend/.")
                .expect("relative path should be valid"),
            "apps/frontend"
        );
        assert_eq!(
            normalize_relative_directory_syntax(".").expect("root should be valid"),
            "."
        );
    }

    #[test]
    fn relative_directory_rejects_parent_components() {
        assert!(matches!(
            normalize_relative_directory_syntax(r"client\..\server"),
            Err(ProjectError::InvalidWorkingDirectory { .. })
        ));
    }

    #[test]
    fn relative_directory_rejects_windows_absolute_and_drive_relative_paths() {
        for path in [r"C:\work\client", "C:client", r"\\server\share\client"] {
            assert!(matches!(
                normalize_relative_directory_syntax(path),
                Err(ProjectError::InvalidWorkingDirectory { .. })
            ));
        }
    }

    #[test]
    #[cfg(windows)]
    fn comparison_key_ignores_case_separator_style_and_trailing_separator() {
        let first = path_key(Path::new(r"C:\Work\Server Dashboard\"));
        let second = path_key(Path::new("c:/work/server dashboard"));

        assert_eq!(first, second);
    }

    #[test]
    #[cfg(windows)]
    fn comparison_key_normalizes_verbatim_paths() {
        let verbatim = path_key(Path::new(r"\\?\C:\Work\Project"));
        let regular = path_key(Path::new(r"c:\work\project"));

        assert_eq!(verbatim, regular);
    }
}
