use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectionResult {
    pub project_id: String,
    pub project_root: String,
    pub suggestions: Vec<ServiceSuggestion>,
    pub warnings: Vec<DetectionWarning>,
    pub scanned_directories: u32,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceSuggestion {
    pub stable_id: String,
    pub display_name: String,
    pub source_kind: SourceKind,
    pub source_path: String,
    pub working_directory: String,
    pub command: String,
    pub reason: String,
    pub default_selected: bool,
    pub editable: bool,
    pub warnings: Vec<DetectionWarning>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SourceKind {
    NpmScript,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectionWarning {
    pub kind: DetectionWarningKind,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DetectionWarningKind {
    InvalidPackageJson,
    EmptyNpmScript,
    UnsafeNpmScriptName,
    NonUtf8Path,
    SymlinkOrJunctionSkipped,
    PermissionDenied,
    FileDisappeared,
    DepthLimitReached,
    DirectoryLimitReached,
    PackageJsonLimitReached,
    SuggestionLimitReached,
    PackageJsonTooLarge,
}
