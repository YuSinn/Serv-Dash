export interface ServiceDefinition {
  id: string;
  name: string;
  workingDirectory: string;
  command: string;
  expectedPort: number | null;
  localUrl: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ServiceInput {
  name: string;
  workingDirectory: string;
  command: string;
  expectedPort: number | null;
  localUrl: string | null;
}

export interface DetectedServiceSubmission {
  stableId: string;
  service: ServiceInput;
}

export type DetectedServiceSkipKind =
  | 'invalidServiceName'
  | 'invalidWorkingDirectory'
  | 'invalidCommand'
  | 'invalidExpectedPort'
  | 'invalidLocalUrl'
  | 'duplicateExistingName'
  | 'duplicateExistingWorkingDirectoryCommand'
  | 'duplicateBatchName'
  | 'duplicateBatchWorkingDirectoryCommand';

export interface AddedDetectedService {
  stableId: string;
  service: ServiceDefinition;
}

export interface SkippedDetectedService {
  stableId: string;
  name: string;
  kind: DetectedServiceSkipKind;
  message: string;
}

export interface AddDetectedServicesResult {
  added: AddedDetectedService[];
  skipped: SkippedDetectedService[];
  services: ServiceDefinition[];
}

export interface Project {
  id: string;
  name: string;
  rootPath: string;
  createdAt: string;
  updatedAt: string;
  services: ServiceDefinition[];
}

export type DetectionSourceKind = 'npmScript' | 'powerShell' | 'cmd' | 'bat';

export type DetectionWarningKind =
  | 'invalidPackageJson'
  | 'emptyNpmScript'
  | 'unsafeNpmScriptName'
  | 'nonUtf8Path'
  | 'symlinkOrJunctionSkipped'
  | 'permissionDenied'
  | 'fileDisappeared'
  | 'depthLimitReached'
  | 'directoryLimitReached'
  | 'packageJsonLimitReached'
  | 'suggestionLimitReached'
  | 'packageJsonTooLarge'
  | 'unsafeWindowsScriptName'
  | 'matchesRegisteredService'
  | 'windowsScriptLimitReached';

export interface DetectionWarning {
  kind: DetectionWarningKind;
  message: string;
  path: string | null;
}

export interface ServiceSuggestion {
  stableId: string;
  displayName: string;
  sourceKind: DetectionSourceKind;
  sourcePath: string;
  workingDirectory: string;
  command: string;
  reason: string;
  defaultSelected: boolean;
  editable: boolean;
  warnings: DetectionWarning[];
}

export interface DetectionResult {
  projectId: string;
  projectRoot: string;
  suggestions: ServiceSuggestion[];
  warnings: DetectionWarning[];
  scannedDirectories: number;
  truncated: boolean;
}

export type ServiceRuntimeStatus =
  'stopped' | 'starting' | 'running' | 'stopping' | 'exited' | 'failed';

export interface ServiceRuntime {
  projectId: string;
  serviceId: string;
  runId: string | null;
  runtimeRevision: number;
  status: ServiceRuntimeStatus;
  pid: number | null;
  startedAt: string | null;
  exitCode: number | null;
  error: string | null;
}

export interface ServiceStartPreview {
  projectId: string;
  projectName: string;
  serviceId: string;
  serviceName: string;
  resolvedWorkingDirectory: string;
  command: string;
}

export type ServiceLogSource = 'stdout' | 'stderr';

export interface ServiceLogEntry {
  sequence: number;
  timestamp: string;
  source: ServiceLogSource;
  text: string;
}

export interface ServiceLogEvent {
  projectId: string;
  serviceId: string;
  runId: string;
  logsRevision: number;
  entry: ServiceLogEntry;
}

export interface ServiceLogsSnapshot {
  projectId: string;
  serviceId: string;
  runId: string | null;
  logsRevision: number;
  entries: ServiceLogEntry[];
}

export interface ProjectsController {
  projects: Project[];
  isLoading: boolean;
  isMutating: boolean;
  error: string | null;
  clearError: () => void;
  isProjectActive: (projectId: string) => boolean;
  addProject: (rootPath: string, name: string) => Promise<boolean>;
  renameProject: (projectId: string, name: string) => Promise<boolean>;
  removeProject: (projectId: string) => Promise<boolean>;
  openProjectFolder: (projectId: string) => Promise<boolean>;
}

export interface ServicesController {
  services: ServiceDefinition[];
  isLoading: boolean;
  isMutating: boolean;
  error: string | null;
  clearError: () => void;
  addService: (service: ServiceInput) => Promise<boolean>;
  updateService: (serviceId: string, service: ServiceInput) => Promise<boolean>;
  removeService: (serviceId: string) => Promise<boolean>;
  resolveWorkingDirectory: (selectedPath: string) => Promise<string | null>;
}

export interface ServiceRuntimeController {
  runtimes: Record<string, ServiceRuntime>;
  busyServiceIds: ReadonlySet<string>;
  selectedServiceId: string | null;
  selectedLogs: ServiceLogsSnapshot | null;
  error: string | null;
  clearError: () => void;
  runtimeFor: (serviceId: string) => ServiceRuntime;
  prepareStart: (serviceId: string) => Promise<ServiceStartPreview | null>;
  start: (serviceId: string) => Promise<boolean>;
  stop: (serviceId: string) => Promise<boolean>;
  selectLogs: (serviceId: string | null) => void;
  clearLogs: (serviceId: string) => Promise<boolean>;
}

export function isRuntimeActive(status: ServiceRuntimeStatus): boolean {
  return ['starting', 'running', 'stopping'].includes(status);
}

export function stoppedRuntime(projectId: string, serviceId: string): ServiceRuntime {
  return {
    projectId,
    serviceId,
    runId: null,
    runtimeRevision: 0,
    status: 'stopped',
    pid: null,
    startedAt: null,
    exitCode: null,
    error: null,
  };
}
