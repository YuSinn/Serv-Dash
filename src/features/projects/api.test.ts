import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { addDetectedServices, detectProjectServices, ProjectsApiError } from './api';
import type {
  AddDetectedServicesResult,
  DetectedServiceSkipKind,
  DetectedServiceSubmission,
  DetectionResult,
  DetectionSourceKind,
  DetectionWarningKind,
  ServiceDefinition,
} from './types';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const sourceKinds = [
  'npmScript',
  'powerShell',
  'cmd',
  'bat',
] as const satisfies readonly DetectionSourceKind[];
const warningKindCoverage = {
  invalidPackageJson: true,
  emptyNpmScript: true,
  unsafeNpmScriptName: true,
  nonUtf8Path: true,
  symlinkOrJunctionSkipped: true,
  permissionDenied: true,
  fileDisappeared: true,
  depthLimitReached: true,
  directoryLimitReached: true,
  packageJsonLimitReached: true,
  suggestionLimitReached: true,
  packageJsonTooLarge: true,
  unsafeWindowsScriptName: true,
  matchesRegisteredService: true,
  windowsScriptLimitReached: true,
} satisfies Record<DetectionWarningKind, true>;

const detectionFixture = {
  projectId: 'project-123',
  projectRoot: 'D:\\work\\project',
  suggestions: [
    {
      stableId: 'npm-id',
      displayName: 'npm: dev',
      sourceKind: 'npmScript',
      sourcePath: 'package.json',
      workingDirectory: '.',
      command: 'npm run -- dev',
      reason: 'npm script declared in package.json',
      defaultSelected: false,
      editable: true,
      warnings: [
        {
          kind: 'matchesRegisteredService',
          message: 'Already registered.',
          path: 'package.json',
        },
      ],
    },
    {
      stableId: 'powershell-id',
      displayName: 'PowerShell: tools.ps1',
      sourceKind: 'powerShell',
      sourcePath: 'tools.ps1',
      workingDirectory: '.',
      command: 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\\tools.ps1"',
      reason: 'Windows PowerShell script file found in the project',
      defaultSelected: false,
      editable: true,
      warnings: [],
    },
    {
      stableId: 'cmd-id',
      displayName: 'CMD: serve.cmd',
      sourceKind: 'cmd',
      sourcePath: 'serve.cmd',
      workingDirectory: '.',
      command: '".\\serve.cmd"',
      reason: 'Windows CMD script file found in the project',
      defaultSelected: false,
      editable: true,
      warnings: [],
    },
    {
      stableId: 'bat-id',
      displayName: 'scripts — BAT: build.bat',
      sourceKind: 'bat',
      sourcePath: 'scripts/build.bat',
      workingDirectory: 'scripts',
      command: '".\\build.bat"',
      reason: 'Windows BAT script file found in the project',
      defaultSelected: false,
      editable: true,
      warnings: [],
    },
  ],
  warnings: [
    {
      kind: 'invalidPackageJson',
      message: 'Invalid JSON.',
      path: 'broken/package.json',
    },
    {
      kind: 'unsafeNpmScriptName',
      message: 'Unsafe npm script.',
      path: 'package.json',
    },
    {
      kind: 'unsafeWindowsScriptName',
      message: 'Unsafe Windows script.',
      path: 'bad&name.cmd',
    },
    {
      kind: 'windowsScriptLimitReached',
      message: 'Windows script limit reached.',
      path: null,
    },
  ],
  scannedDirectories: 27,
  truncated: true,
} satisfies DetectionResult;

const detectedSkipKindCoverage = {
  invalidServiceName: true,
  invalidWorkingDirectory: true,
  invalidCommand: true,
  invalidExpectedPort: true,
  invalidLocalUrl: true,
  duplicateExistingName: true,
  duplicateExistingWorkingDirectoryCommand: true,
  duplicateBatchName: true,
  duplicateBatchWorkingDirectoryCommand: true,
} satisfies Record<DetectedServiceSkipKind, true>;

const detectedSubmissions = [
  {
    stableId: '  opaque-a  ',
    service: {
      name: 'API',
      workingDirectory: 'apps/api',
      command: 'npm run -- dev',
      expectedPort: 3_000,
      localUrl: 'http://localhost:3000',
    },
  },
  {
    stableId: 'opaque-b',
    service: {
      name: 'Worker',
      workingDirectory: '.',
      command: 'npm run -- worker',
      expectedPort: null,
      localUrl: null,
    },
  },
] satisfies readonly DetectedServiceSubmission[];

function serviceDefinition(id: string, name: string): ServiceDefinition {
  return {
    id,
    name,
    workingDirectory: '.',
    command: `run ${name}`,
    expectedPort: null,
    localUrl: null,
    createdAt: '2026-07-31T00:00:00.000Z',
    updatedAt: '2026-07-31T00:00:00.000Z',
  };
}

const addedServiceA = serviceDefinition('service-a', 'API');
const addedServiceB = serviceDefinition('service-b', 'Worker');
const batchResult = {
  added: [
    { stableId: '  opaque-a  ', service: addedServiceA },
    { stableId: 'opaque-b', service: addedServiceB },
  ],
  skipped: [
    {
      stableId: 'duplicate-a',
      name: 'Duplicate A',
      kind: 'duplicateExistingName',
      message: 'A service with this name already exists.',
    },
    {
      stableId: 'invalid-b',
      name: 'Invalid B',
      kind: 'invalidCommand',
      message: 'Enter a valid command.',
    },
  ],
  services: [addedServiceA, addedServiceB],
} satisfies AddDetectedServicesResult;

beforeEach(() => {
  invokeMock.mockReset();
});

describe('detectProjectServices', () => {
  it('passes the exact payload through and preserves the complete detection result', async () => {
    const before = JSON.stringify(detectionFixture);
    invokeMock.mockResolvedValue(detectionFixture);

    const result = await detectProjectServices('project-123');

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('detect_project_services', {
      projectId: 'project-123',
    });
    expect(invokeMock.mock.calls[0]?.[1]).not.toHaveProperty('projectRoot');
    expect(invokeMock.mock.calls.map(([command]) => command)).not.toContain('add_service');
    expect(result).toBe(detectionFixture);
    expect(result.suggestions.map(({ sourceKind }) => sourceKind)).toEqual(sourceKinds);
    expect(result.warnings).toBe(detectionFixture.warnings);
    expect(result.suggestions[0]?.warnings).toBe(detectionFixture.suggestions[0].warnings);
    expect(result.warnings[3]?.path).toBeNull();
    expect(result.scannedDirectories).toBe(27);
    expect(result.truncated).toBe(true);
    expect(JSON.stringify(detectionFixture)).toBe(before);
    expect(Object.keys(warningKindCoverage)).toHaveLength(15);
  });

  it('maps a structured Tauri rejection through ProjectsApiError', async () => {
    invokeMock.mockRejectedValue({ code: 'project_not_found', message: 'Project missing.' });

    const error = await detectProjectServices('missing').catch((value: unknown) => value);

    expect(error).toBeInstanceOf(ProjectsApiError);
    expect(error).toMatchObject({
      name: 'ProjectsApiError',
      code: 'project_not_found',
      message: 'Project missing.',
    });
  });

  it('rethrows an unknown Tauri rejection unchanged', async () => {
    const rejection = { reason: 'IPC unavailable' };
    invokeMock.mockRejectedValue(rejection);

    await expect(detectProjectServices('project-123')).rejects.toBe(rejection);
  });
});

describe('addDetectedServices', () => {
  it('passes the exact ordered batch through and returns the backend result unchanged', async () => {
    invokeMock.mockResolvedValue(batchResult);

    const result = await addDetectedServices('project-123', detectedSubmissions);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('add_detected_services', {
      projectId: 'project-123',
      submissions: detectedSubmissions,
    });
    expect(detectedSubmissions.map(({ stableId }) => stableId)).toEqual([
      '  opaque-a  ',
      'opaque-b',
    ]);
    expect(detectedSubmissions[0]?.service).toEqual({
      name: 'API',
      workingDirectory: 'apps/api',
      command: 'npm run -- dev',
      expectedPort: 3_000,
      localUrl: 'http://localhost:3000',
    });
    expect(detectedSubmissions[1]?.service.expectedPort).toBeNull();
    expect(detectedSubmissions[1]?.service.localUrl).toBeNull();
    expect(detectedSubmissions[0]?.service).not.toHaveProperty('stableId');
    expect(result).toBe(batchResult);
    expect(result.added).toBe(batchResult.added);
    expect(result.skipped).toBe(batchResult.skipped);
    expect(result.services).toBe(batchResult.services);
    expect(result.added.map(({ stableId }) => stableId)).toEqual(['  opaque-a  ', 'opaque-b']);
    expect(result.skipped.map(({ stableId }) => stableId)).toEqual(['duplicate-a', 'invalid-b']);
    expect(result.services.map(({ id }) => id)).toEqual(['service-a', 'service-b']);
    expect(Object.keys(detectedSkipKindCoverage)).toHaveLength(9);
  });

  it('preserves a structured global rejection without retrying', async () => {
    invokeMock.mockRejectedValue({
      code: 'persistence_failed',
      message: 'Project data could not be saved.',
    });

    const error = await addDetectedServices('project-123', detectedSubmissions).catch(
      (value: unknown) => value,
    );

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(error).toBeInstanceOf(ProjectsApiError);
    expect(error).toMatchObject({
      code: 'persistence_failed',
      message: 'Project data could not be saved.',
    });
  });
});
