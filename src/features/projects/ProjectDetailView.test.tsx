import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ProjectDetailView } from './ProjectDetailView';
import {
  type AddDetectedServicesResult,
  stoppedRuntime,
  type Project,
  type ServiceDefinition,
  type ServiceRuntimeController,
} from './types';
import type { UseDetectedServiceSubmissionResult } from './useDetectedServiceSubmission';
import { useServiceRuntime } from './useServiceRuntime';
import { useServices, type UseServicesResult } from './useServices';

const { serviceDetectionControlMock } = vi.hoisted(() => ({
  serviceDetectionControlMock: vi.fn((_props: unknown) => null),
}));

vi.mock('./ServiceDetectionControl', () => ({
  ServiceDetectionControl: serviceDetectionControlMock,
}));
vi.mock('./ServiceCard', () => ({
  ServiceCard: ({ service }: { service: ServiceDefinition }) => (
    <div data-testid="canonical-service-card">{service.name}</div>
  ),
}));
vi.mock('./ServiceLogsPanel', () => ({ ServiceLogsPanel: () => null }));
vi.mock('./useServiceRuntime', () => ({ useServiceRuntime: vi.fn() }));
vi.mock('./useServices', () => ({ useServices: vi.fn() }));

const useServicesMock = vi.mocked(useServices);
const useServiceRuntimeMock = vi.mocked(useServiceRuntime);

function service(id: string, name: string): ServiceDefinition {
  return {
    id,
    name,
    workingDirectory: '.',
    command: `run ${id}`,
    expectedPort: null,
    localUrl: null,
    createdAt: '2026-07-31T00:00:00Z',
    updatedAt: '2026-07-31T00:00:00Z',
  };
}

function submission(
  overrides: Partial<UseDetectedServiceSubmissionResult> = {},
): UseDetectedServiceSubmissionResult {
  return {
    status: 'idle',
    result: null,
    error: null,
    submit: vi.fn().mockResolvedValue(true),
    reset: vi.fn(),
    ...overrides,
  };
}

function servicesController(
  services: ServiceDefinition[],
  detectedSubmission: UseDetectedServiceSubmissionResult,
): UseServicesResult {
  return {
    services,
    isLoading: false,
    isMutating: false,
    error: null,
    clearError: vi.fn(),
    addService: vi.fn().mockResolvedValue(true),
    updateService: vi.fn().mockResolvedValue(true),
    removeService: vi.fn().mockResolvedValue(true),
    resolveWorkingDirectory: vi.fn().mockResolvedValue(null),
    detectedSubmission,
  };
}

const runtimeController: ServiceRuntimeController = {
  runtimes: {},
  busyServiceIds: new Set(),
  selectedServiceId: null,
  selectedLogs: null,
  error: null,
  clearError: vi.fn(),
  runtimeFor: vi.fn((serviceId: string) => stoppedRuntime('project-a', serviceId)),
  prepareStart: vi.fn().mockResolvedValue(null),
  start: vi.fn().mockResolvedValue(false),
  stop: vi.fn().mockResolvedValue(false),
  selectLogs: vi.fn(),
  clearLogs: vi.fn().mockResolvedValue(false),
};

const project: Project = {
  id: 'project-a',
  name: 'Project A',
  rootPath: 'D:\\projects\\project-a',
  createdAt: '2026-07-31T00:00:00Z',
  updatedAt: '2026-07-31T00:00:00Z',
  services: [],
};

beforeEach(() => {
  serviceDetectionControlMock.mockClear();
  useServicesMock.mockReset();
  useServiceRuntimeMock.mockReturnValue(runtimeController);
});

afterEach(cleanup);

describe('ProjectDetailView detected-service wiring', () => {
  it('renders service cards exclusively from the canonical controller list', () => {
    const canonical = service('canonical', 'Canonical service');
    const resultAddedDecoy = service('result-added', 'Result-added decoy');
    const resultServicesDecoy = service('result-services', 'Result-services decoy');
    const result: AddDetectedServicesResult = {
      added: [{ stableId: 'decoy', service: resultAddedDecoy }],
      skipped: [],
      services: [resultServicesDecoy],
    };
    const detectedSubmission = submission({ status: 'success', result });
    useServicesMock.mockReturnValue(servicesController([canonical], detectedSubmission));

    render(
      <ProjectDetailView
        project={project}
        projectError={null}
        isProjectMutating={false}
        onBack={vi.fn()}
        onClearProjectError={vi.fn()}
        onOpenRoot={vi.fn()}
      />,
    );

    const cards = screen.getAllByTestId('canonical-service-card');
    expect(cards).toHaveLength(1);
    expect(cards[0]).toHaveTextContent('Canonical service');
    expect(cards.some((card) => card.textContent === 'Result-added decoy')).toBe(false);
    expect(cards.some((card) => card.textContent === 'Result-services decoy')).toBe(false);
  });

  it('renders and passes through only the canonical controller services', () => {
    const detectedSubmission = submission();
    const original = service('original', 'Original service');
    const authoritative = service('authoritative', 'Authoritative service');
    useServicesMock.mockReturnValue(servicesController([original], detectedSubmission));
    const { rerender } = render(
      <ProjectDetailView
        project={project}
        projectError={null}
        isProjectMutating={false}
        onBack={vi.fn()}
        onClearProjectError={vi.fn()}
        onOpenRoot={vi.fn()}
      />,
    );
    expect(screen.getByTestId('canonical-service-card')).toHaveTextContent('Original service');

    useServicesMock.mockReturnValue(servicesController([authoritative], detectedSubmission));
    rerender(
      <ProjectDetailView
        project={project}
        projectError={null}
        isProjectMutating={false}
        onBack={vi.fn()}
        onClearProjectError={vi.fn()}
        onOpenRoot={vi.fn()}
      />,
    );

    expect(screen.getByTestId('canonical-service-card')).toHaveTextContent('Authoritative service');
    expect(screen.queryByText('Original service')).not.toBeInTheDocument();
    const latestDetectionProps = serviceDetectionControlMock.mock.lastCall?.[0];
    expect(latestDetectionProps).toEqual(
      expect.objectContaining({
        projectId: 'project-a',
        services: [authoritative],
        submission: detectedSubmission,
        isBusy: false,
      }),
    );
  });
});
