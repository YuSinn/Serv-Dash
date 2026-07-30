use super::model::{DetectionWarning, DetectionWarningKind, ServiceSuggestion, SourceKind};
use super::node::DETECTION_NAMESPACE;
use std::path::Path;
use uuid::Uuid;

pub(super) struct WindowsScriptInspection {
    pub suggestion: Option<ServiceSuggestion>,
    pub warnings: Vec<DetectionWarning>,
}

pub(super) fn source_kind(file_name: &str) -> Option<SourceKind> {
    match Path::new(file_name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "ps1" => Some(SourceKind::PowerShell),
        "cmd" => Some(SourceKind::Cmd),
        "bat" => Some(SourceKind::Bat),
        _ => None,
    }
}

pub(super) fn inspect_windows_script(
    project_id: &str,
    kind: SourceKind,
    source_path: &str,
    working_directory: &str,
    file_name: &str,
) -> WindowsScriptInspection {
    if !is_safe_windows_script_name(file_name) {
        return WindowsScriptInspection {
            suggestion: None,
            warnings: vec![DetectionWarning {
                kind: DetectionWarningKind::UnsafeWindowsScriptName,
                message: format!(
                    "The Windows script name '{file_name}' cannot be represented safely."
                ),
                path: Some(source_path.to_owned()),
            }],
        };
    }

    let label = label(kind);
    let relative_command_path = format!(".\\{file_name}");
    let quoted_path = format!(
        "{}{}{}",
        char::from(34),
        relative_command_path,
        char::from(34)
    );
    let command = match kind {
        SourceKind::PowerShell => {
            format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -File {quoted_path}")
        }
        SourceKind::Cmd | SourceKind::Bat => quoted_path,
        SourceKind::NpmScript => {
            return WindowsScriptInspection {
                suggestion: None,
                warnings: Vec::new(),
            };
        }
    };

    WindowsScriptInspection {
        suggestion: Some(ServiceSuggestion {
            stable_id: stable_id(project_id, kind, source_path, file_name),
            display_name: if working_directory == "." {
                format!("{label}: {file_name}")
            } else {
                format!("{working_directory} \u{2014} {label}: {file_name}")
            },
            source_kind: kind,
            source_path: source_path.to_owned(),
            working_directory: working_directory.to_owned(),
            command,
            reason: format!("Windows {label} script file found in the project"),
            default_selected: false,
            editable: true,
            warnings: Vec::new(),
        }),
        warnings: Vec::new(),
    }
}

fn is_safe_windows_script_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.')
        })
}

fn label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::PowerShell => "PowerShell",
        SourceKind::Cmd => "CMD",
        SourceKind::Bat => "BAT",
        SourceKind::NpmScript => "npm",
    }
}

fn stable_id(project_id: &str, kind: SourceKind, source_path: &str, file_name: &str) -> String {
    let discriminator = format!(
        "{project_id}\0{}\0{}\0{file_name}",
        stable_kind(kind),
        source_path.replace('\\', "/").to_lowercase()
    );
    Uuid::new_v5(&DETECTION_NAMESPACE, discriminator.as_bytes()).to_string()
}

fn stable_kind(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::PowerShell => "PowerShell",
        SourceKind::Cmd => "Cmd",
        SourceKind::Bat => "Bat",
        SourceKind::NpmScript => "NpmScript",
    }
}
