import { invoke } from '@tauri-apps/api/core';
import type {
  DetectionResult,
  Project,
  ServiceDefinition,
  ServiceInput,
  ServiceLogsSnapshot,
  ServiceRuntime,
  ServiceStartPreview,
} from './types';

export const SERVICE_RUNTIME_EVENT = 'service-runtime-updated';
export const SERVICE_LOG_EVENT = 'service-log-appended';
export const SERVICE_LOGS_CLEARED_EVENT = 'service-logs-cleared';

interface BackendErrorPayload {
  code: string;
  message: string;
}

export class ProjectsApiError extends Error {
  readonly code: string;

  constructor(payload: BackendErrorPayload) {
    super(payload.message);
    this.name = 'ProjectsApiError';
    this.code = payload.code;
  }
}

function errorPayload(value: unknown): BackendErrorPayload | null {
  if (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'message' in value &&
    typeof value.code === 'string' &&
    typeof value.message === 'string'
  ) {
    return { code: value.code, message: value.message };
  }

  if (typeof value === 'string') {
    try {
      return errorPayload(JSON.parse(value));
    } catch {
      return null;
    }
  }

  return null;
}

export function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  const payload = errorPayload(error);
  if (payload) {
    return payload.message;
  }

  return typeof error === 'string' ? error : 'An unexpected error occurred.';
}

async function invokeCommand<T>(
  command: string,
  arguments_: Record<string, unknown> = {},
): Promise<T> {
  try {
    return await invoke<T>(command, arguments_);
  } catch (error) {
    const payload = errorPayload(error);
    if (payload) {
      throw new ProjectsApiError(payload);
    }
    throw error;
  }
}

export function listProjects(): Promise<Project[]> {
  return invokeCommand<Project[]>('list_projects');
}

export function addProject(rootPath: string, name: string): Promise<Project[]> {
  return invokeCommand<Project[]>('add_project', { rootPath, name });
}

export function renameProject(projectId: string, name: string): Promise<Project[]> {
  return invokeCommand<Project[]>('rename_project', { projectId, name });
}

export function removeProject(projectId: string): Promise<Project[]> {
  return invokeCommand<Project[]>('remove_project', { projectId });
}

export function openProjectFolder(projectId: string): Promise<void> {
  return invokeCommand<void>('open_project_folder', { projectId });
}

export function listServices(projectId: string): Promise<ServiceDefinition[]> {
  return invokeCommand<ServiceDefinition[]>('list_services', { projectId });
}

export function detectProjectServices(projectId: string): Promise<DetectionResult> {
  return invokeCommand<DetectionResult>('detect_project_services', { projectId });
}

export function addService(projectId: string, service: ServiceInput): Promise<ServiceDefinition[]> {
  return invokeCommand<ServiceDefinition[]>('add_service', {
    projectId,
    service,
  });
}

export function updateService(
  projectId: string,
  serviceId: string,
  service: ServiceInput,
): Promise<ServiceDefinition[]> {
  return invokeCommand<ServiceDefinition[]>('update_service', {
    projectId,
    serviceId,
    service,
  });
}

export function removeService(projectId: string, serviceId: string): Promise<ServiceDefinition[]> {
  return invokeCommand<ServiceDefinition[]>('remove_service', {
    projectId,
    serviceId,
  });
}

export function resolveServiceDirectory(projectId: string, selectedPath: string): Promise<string> {
  return invokeCommand<string>('resolve_service_directory', {
    projectId,
    selectedPath,
  });
}

export function getServiceStartPreview(
  projectId: string,
  serviceId: string,
): Promise<ServiceStartPreview> {
  return invokeCommand<ServiceStartPreview>('get_service_start_preview', {
    projectId,
    serviceId,
  });
}

export function startService(projectId: string, serviceId: string): Promise<ServiceRuntime> {
  return invokeCommand<ServiceRuntime>('start_service', {
    projectId,
    serviceId,
  });
}

export function stopService(projectId: string, serviceId: string): Promise<ServiceRuntime> {
  return invokeCommand<ServiceRuntime>('stop_service', {
    projectId,
    serviceId,
  });
}

export function getServiceRuntime(projectId: string, serviceId: string): Promise<ServiceRuntime> {
  return invokeCommand<ServiceRuntime>('get_service_runtime', {
    projectId,
    serviceId,
  });
}

export function getServiceLogs(projectId: string, serviceId: string): Promise<ServiceLogsSnapshot> {
  return invokeCommand<ServiceLogsSnapshot>('get_service_logs', {
    projectId,
    serviceId,
  });
}

export function clearServiceLogs(
  projectId: string,
  serviceId: string,
): Promise<ServiceLogsSnapshot> {
  return invokeCommand<ServiceLogsSnapshot>('clear_service_logs', {
    projectId,
    serviceId,
  });
}
