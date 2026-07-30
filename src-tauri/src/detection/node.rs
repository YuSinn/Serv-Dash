use super::model::{DetectionWarning, DetectionWarningKind, ServiceSuggestion, SourceKind};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use uuid::Uuid;

// Fixed forever: changing it would invalidate user choices keyed by stable_id.
pub(super) const DETECTION_NAMESPACE: Uuid =
    Uuid::from_u128(0x8f6f_7517_6f46_58e0_9bb8_42e8_211f_8dcc);

pub(super) struct PackageInspection {
    pub suggestions: Vec<ServiceSuggestion>,
    pub warnings: Vec<DetectionWarning>,
}

pub(super) struct PackageContext<'a> {
    pub project_id: &'a str,
    pub package_path: &'a Path,
    pub source_path: &'a str,
    pub working_directory: &'a str,
    pub max_bytes: u64,
}

pub(super) fn inspect_package(context: PackageContext<'_>) -> PackageInspection {
    let bytes = match read_bounded(context.package_path, context.max_bytes, context.source_path) {
        Ok(bytes) => bytes,
        Err(warning) => {
            return PackageInspection {
                suggestions: Vec::new(),
                warnings: vec![warning],
            };
        }
    };
    parse_package(&bytes, &context)
}

fn read_bounded(
    package_path: &Path,
    max_bytes: u64,
    source_path: &str,
) -> Result<Vec<u8>, DetectionWarning> {
    let file = File::open(package_path)
        .map_err(|error| io_warning(&error, "read package.json", source_path))?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_warning(&error, "read package.json", source_path))?;
    if bytes.len() as u64 > max_bytes {
        Err(DetectionWarning {
            kind: DetectionWarningKind::PackageJsonTooLarge,
            message: format!("package.json exceeds the {max_bytes}-byte size limit."),
            path: Some(source_path.to_owned()),
        })
    } else {
        Ok(bytes)
    }
}

fn parse_package(bytes: &[u8], context: &PackageContext<'_>) -> PackageInspection {
    let value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => {
            return invalid_manifest(context.source_path, format!("Invalid JSON: {error}"));
        }
    };
    let Some(root) = value.as_object() else {
        return invalid_manifest(
            context.source_path,
            "The JSON root must be an object.".to_owned(),
        );
    };
    let Some(scripts_value) = root.get("scripts") else {
        return PackageInspection {
            suggestions: Vec::new(),
            warnings: Vec::new(),
        };
    };
    let Some(scripts) = scripts_value.as_object() else {
        return invalid_manifest(
            context.source_path,
            "The scripts property must be an object.".to_owned(),
        );
    };

    let mut entries: Vec<_> = scripts.iter().collect();
    entries.sort_by(|(left, _), (right, _)| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    let mut inspection = PackageInspection {
        suggestions: Vec::new(),
        warnings: Vec::new(),
    };
    for (script_name, script_value) in entries {
        let Some(script_value) = script_value.as_str() else {
            continue;
        };
        if script_value.trim().is_empty() {
            inspection.warnings.push(DetectionWarning {
                kind: DetectionWarningKind::EmptyNpmScript,
                message: format!("The npm script '{script_name}' has an empty command."),
                path: Some(context.source_path.to_owned()),
            });
            continue;
        }
        if !is_safe_script_name(script_name) {
            inspection.warnings.push(DetectionWarning {
                kind: DetectionWarningKind::UnsafeNpmScriptName,
                message: format!(
                    "The npm script name '{script_name}' cannot be represented safely."
                ),
                path: Some(context.source_path.to_owned()),
            });
            continue;
        }

        inspection.suggestions.push(ServiceSuggestion {
            stable_id: stable_id(context.project_id, context.source_path, script_name),
            display_name: if context.working_directory == "." {
                format!("npm: {script_name}")
            } else {
                format!("{} \u{2014} npm: {script_name}", context.working_directory)
            },
            source_kind: SourceKind::NpmScript,
            source_path: context.source_path.to_owned(),
            working_directory: context.working_directory.to_owned(),
            command: if script_name.contains(' ') {
                format!(
                    "npm run -- {}{script_name}{}",
                    char::from(34),
                    char::from(34)
                )
            } else {
                format!("npm run -- {script_name}")
            },
            reason: "npm script declared in package.json".to_owned(),
            default_selected: matches!(
                script_name.to_ascii_lowercase().as_str(),
                "dev" | "start" | "serve" | "preview"
            ),
            editable: true,
            warnings: Vec::new(),
        });
    }
    inspection
}

fn invalid_manifest(source_path: &str, message: String) -> PackageInspection {
    PackageInspection {
        suggestions: Vec::new(),
        warnings: vec![DetectionWarning {
            kind: DetectionWarningKind::InvalidPackageJson,
            message,
            path: Some(source_path.to_owned()),
        }],
    }
}

fn is_safe_script_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_alphanumeric()
                || matches!(character, ' ' | '-' | '_' | '.' | ':' | '/' | '@')
        })
}

fn stable_id(project_id: &str, source_path: &str, script_name: &str) -> String {
    let discriminator = format!(
        "{project_id}\0npm-script\0{}\0{script_name}",
        source_path.to_lowercase()
    );
    Uuid::new_v5(&DETECTION_NAMESPACE, discriminator.as_bytes()).to_string()
}

fn io_warning(error: &io::Error, action: &str, source_path: &str) -> DetectionWarning {
    DetectionWarning {
        kind: if error.kind() == io::ErrorKind::NotFound {
            DetectionWarningKind::FileDisappeared
        } else {
            DetectionWarningKind::PermissionDenied
        },
        message: format!("Could not {action}: {error}"),
        path: Some(source_path.to_owned()),
    }
}
