import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  addDetectedServices,
  addService,
  listServices,
  ProjectsApiError,
  removeService,
  updateService,
} from './api';
import type { PreparedDetectedService } from './detectionSubmission';
import type {
  AddDetectedServicesResult,
  AddedDetectedService,
  ServiceDefinition,
  ServiceInput,
  SkippedDetectedService,
} from './types';
import {
  createServiceMutationCoordinator,
  useDetectedServiceSubmission,
  type ServiceMutationCoordinator,
  type UseDetectedServiceSubmissionResult,
} from './useDetectedServiceSubmission';
import { useServices } from './useServices';

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return {
    ...actual,
    addDetectedServices: vi.fn(),
    addService: vi.fn(),
    listServices: vi.fn(),
    removeService: vi.fn(),
    updateService: vi.fn(),
  };
});

const addDetectedServicesMock = vi.mocked(addDetectedServices);
const addServiceMock = vi.mocked(addService);
const listServicesMock = vi.mocked(listServices);
const removeServiceMock = vi.mocked(removeService);
const updateServiceMock = vi.mocked(updateService);
let mutationCoordinator: ServiceMutationCoordinator;

interface Deferred<T> {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (reason: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function input(name: string, overrides: Partial<ServiceInput> = {}): ServiceInput {
  return {
    name,
    workingDirectory: '.',
    command: `run ${name}`,
    expectedPort: null,
    localUrl: null,
    ...overrides,
  };
}

function prepared(stableId: string, serviceInput = input(stableId)): PreparedDetectedService {
  return { stableId, input: serviceInput };
}

function service(id: string): ServiceDefinition {
  return {
    id,
    name: `Service ${id}`,
    workingDirectory: '.',
    command: `run ${id}`,
    expectedPort: null,
    localUrl: null,
    createdAt: '2026-07-31T00:00:00.000Z',
    updatedAt: '2026-07-31T00:00:00.000Z',
  };
}

function batchResult({
  services,
  added = [],
  skipped = [],
}: {
  readonly services: ServiceDefinition[];
  readonly added?: AddedDetectedService[];
  readonly skipped?: SkippedDetectedService[];
}): AddDetectedServicesResult {
  return { added, skipped, services };
}

async function submitHook(
  hook: { readonly current: UseDetectedServiceSubmissionResult },
  items: readonly PreparedDetectedService[],
): Promise<boolean> {
  let outcome = false;
  await act(async () => {
    outcome = await hook.current.submit(items);
  });
  return outcome;
}

beforeEach(() => {
  addDetectedServicesMock.mockReset();
  addServiceMock.mockReset();
  listServicesMock.mockReset();
  removeServiceMock.mockReset();
  updateServiceMock.mockReset();
  listServicesMock.mockResolvedValue([]);
  mutationCoordinator = createServiceMutationCoordinator();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('useDetectedServiceSubmission state and results', () => {
  it('starts idle and treats an empty submission as a defensive no-op', async () => {
    const replaceServices = vi.fn();
    const { result } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );

    expect(result.current).toMatchObject({ status: 'idle', result: null, error: null });
    expect(await submitHook(result, [])).toBe(false);
    expect(addDetectedServicesMock).not.toHaveBeenCalled();
    expect(replaceServices).not.toHaveBeenCalled();
    expect(result.current).toMatchObject({ status: 'idle', result: null, error: null });
  });

  it('submits once, preserves input identity and order, and applies total success', async () => {
    const pending = deferred<AddDetectedServicesResult>();
    const firstInput = input('first', { expectedPort: 3_000 });
    const secondInput = input('second');
    const items = [prepared('  first  ', firstInput), prepared('second', secondInput)];
    const before = structuredClone(items);
    const services = [service('one'), service('two')];
    const response = batchResult({
      services,
      added: [
        { stableId: '  first  ', service: services[0]! },
        { stableId: 'second', service: services[1]! },
      ],
    });
    addDetectedServicesMock.mockReturnValue(pending.promise);
    const replaceServices = vi.fn();
    const { result } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );
    let submission!: Promise<boolean>;

    act(() => {
      submission = result.current.submit(items);
    });

    expect(result.current.status).toBe('submitting');
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(1);
    expect(addDetectedServicesMock).toHaveBeenCalledWith('project-a', [
      { stableId: '  first  ', service: firstInput },
      { stableId: 'second', service: secondInput },
    ]);
    const sent = addDetectedServicesMock.mock.calls[0]?.[1];
    expect(sent?.[0]?.service).toBe(firstInput);
    expect(sent?.[1]?.service).toBe(secondInput);

    let outcome = false;
    await act(async () => {
      pending.resolve(response);
      outcome = await submission;
    });

    expect(outcome).toBe(true);
    expect(result.current.status).toBe('success');
    expect(result.current.result).toBe(response);
    expect(result.current.error).toBeNull();
    expect(replaceServices).toHaveBeenCalledOnce();
    expect(replaceServices).toHaveBeenCalledWith(services);
    expect(items).toEqual(before);
  });

  it.each([
    {
      label: 'partial success',
      response: batchResult({
        services: [service('partial')],
        added: [{ stableId: 'added', service: service('partial') }],
        skipped: [
          {
            stableId: 'skipped',
            name: 'Skipped',
            kind: 'duplicateExistingName',
            message: 'Duplicate.',
          },
        ],
      }),
    },
    {
      label: 'all skipped',
      response: batchResult({
        services: [service('authoritative-existing')],
        skipped: [
          {
            stableId: 'skipped',
            name: 'Skipped',
            kind: 'invalidCommand',
            message: 'Invalid.',
          },
        ],
      }),
    },
  ])('keeps $label as success and applies authoritative services', async ({ response }) => {
    addDetectedServicesMock.mockResolvedValue(response);
    const replaceServices = vi.fn();
    const { result } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );

    expect(await submitHook(result, [prepared('one')])).toBe(true);

    expect(result.current.status).toBe('success');
    expect(result.current.result).toBe(response);
    expect(replaceServices).toHaveBeenCalledWith(response.services);
  });

  it('preserves a global error and leaves canonical services untouched', async () => {
    const rejection = new ProjectsApiError({
      code: 'persistence_failed',
      message: 'Could not save.',
    });
    addDetectedServicesMock.mockRejectedValue(rejection);
    const replaceServices = vi.fn();
    const { result } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );

    expect(await submitHook(result, [prepared('one')])).toBe(false);

    expect(result.current.status).toBe('error');
    expect(result.current.result).toBeNull();
    expect(result.current.error).toBe(rejection);
    expect(replaceServices).not.toHaveBeenCalled();
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(1);
  });

  it('blocks duplicate submits synchronously and allows another after completion', async () => {
    const pending = deferred<AddDetectedServicesResult>();
    const firstResponse = batchResult({ services: [service('first')] });
    addDetectedServicesMock.mockReturnValueOnce(pending.promise);
    const replaceServices = vi.fn();
    const { result } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );
    let first!: Promise<boolean>;
    let duplicate!: Promise<boolean>;

    act(() => {
      first = result.current.submit([prepared('first')]);
      duplicate = result.current.submit([prepared('duplicate')]);
    });

    await expect(duplicate).resolves.toBe(false);
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      pending.resolve(firstResponse);
      await first;
    });

    const secondResponse = batchResult({ services: [service('second')] });
    addDetectedServicesMock.mockResolvedValueOnce(secondResponse);
    expect(await submitHook(result, [prepared('second')])).toBe(true);
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(2);
    expect(result.current.result).toBe(secondResponse);
  });

  it('keeps reset defensive while submitting and clears completed state afterward', async () => {
    const pending = deferred<AddDetectedServicesResult>();
    const response = batchResult({ services: [service('one')] });
    addDetectedServicesMock.mockReturnValue(pending.promise);
    const replaceServices = vi.fn();
    const { result } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );
    let submission!: Promise<boolean>;
    act(() => {
      submission = result.current.submit([prepared('one')]);
      result.current.reset();
    });
    expect(result.current.status).toBe('submitting');

    await act(async () => {
      pending.resolve(response);
      await submission;
    });
    expect(result.current.status).toBe('success');

    act(() => result.current.reset());
    expect(result.current).toMatchObject({ status: 'idle', result: null, error: null });
    expect(replaceServices).toHaveBeenCalledTimes(1);
  });
});

describe('useDetectedServiceSubmission stale request ownership', () => {
  it('keeps the new request lock when obsolete A settles during pending B', async () => {
    const pendingA = deferred<AddDetectedServicesResult>();
    const pendingB = deferred<AddDetectedServicesResult>();
    addDetectedServicesMock
      .mockReturnValueOnce(pendingA.promise)
      .mockReturnValueOnce(pendingB.promise);
    const replaceServices = vi.fn();
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) =>
        useDetectedServiceSubmission(projectId, replaceServices, mutationCoordinator),
      { initialProps: { projectId: 'project-a' } },
    );
    let submitA!: Promise<boolean>;
    act(() => {
      submitA = result.current.submit([prepared('a')]);
    });

    rerender({ projectId: 'project-b' });
    expect(result.current.status).toBe('idle');
    let submitB!: Promise<boolean>;
    act(() => {
      submitB = result.current.submit([prepared('b')]);
    });
    expect(result.current.status).toBe('submitting');
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(2);

    let outcomeA = true;
    await act(async () => {
      pendingA.resolve(batchResult({ services: [service('from-a')] }));
      outcomeA = await submitA;
    });
    expect(outcomeA).toBe(false);
    expect(result.current.status).toBe('submitting');
    expect(replaceServices).not.toHaveBeenCalled();

    let outcomeC = true;
    await act(async () => {
      outcomeC = await result.current.submit([prepared('c')]);
    });
    expect(outcomeC).toBe(false);
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(2);
    expect(result.current.status).toBe('submitting');

    const responseB = batchResult({ services: [service('from-b')] });
    let outcomeB = false;
    await act(async () => {
      pendingB.resolve(responseB);
      outcomeB = await submitB;
    });
    expect(outcomeB).toBe(true);
    expect(result.current.result).toBe(responseB);
    expect(replaceServices).toHaveBeenCalledOnce();
    expect(replaceServices).toHaveBeenCalledWith(responseB.services);

    const responseD = batchResult({ services: [service('from-d')] });
    addDetectedServicesMock.mockResolvedValueOnce(responseD);
    expect(await submitHook(result, [prepared('d')])).toBe(true);
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(3);
    expect(result.current.result).toBe(responseD);
  });

  it('does not let an old result overwrite a completed new-project result', async () => {
    const pendingA = deferred<AddDetectedServicesResult>();
    const pendingB = deferred<AddDetectedServicesResult>();
    addDetectedServicesMock
      .mockReturnValueOnce(pendingA.promise)
      .mockReturnValueOnce(pendingB.promise);
    const replaceServices = vi.fn();
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) =>
        useDetectedServiceSubmission(projectId, replaceServices, mutationCoordinator),
      { initialProps: { projectId: 'project-a' } },
    );
    let submitA!: Promise<boolean>;
    act(() => {
      submitA = result.current.submit([prepared('a')]);
    });
    rerender({ projectId: 'project-b' });
    let submitB!: Promise<boolean>;
    act(() => {
      submitB = result.current.submit([prepared('b')]);
    });
    const responseB = batchResult({ services: [service('from-b')] });
    await act(async () => {
      pendingB.resolve(responseB);
      await submitB;
    });

    let outcomeA = true;
    await act(async () => {
      pendingA.resolve(batchResult({ services: [service('late-a')] }));
      outcomeA = await submitA;
    });

    expect(outcomeA).toBe(false);
    expect(result.current.status).toBe('success');
    expect(result.current.result).toBe(responseB);
    expect(replaceServices).toHaveBeenCalledTimes(1);
    expect(replaceServices).toHaveBeenCalledWith(responseB.services);
  });

  it('ignores an old error after project change', async () => {
    const pending = deferred<AddDetectedServicesResult>();
    addDetectedServicesMock.mockReturnValue(pending.promise);
    const replaceServices = vi.fn();
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) =>
        useDetectedServiceSubmission(projectId, replaceServices, mutationCoordinator),
      { initialProps: { projectId: 'project-a' } },
    );
    let submission!: Promise<boolean>;
    act(() => {
      submission = result.current.submit([prepared('a')]);
    });

    rerender({ projectId: 'project-b' });
    let outcome = true;
    await act(async () => {
      pending.reject(new Error('late failure'));
      outcome = await submission;
    });

    expect(outcome).toBe(false);
    expect(result.current).toMatchObject({ status: 'idle', result: null, error: null });
    expect(replaceServices).not.toHaveBeenCalled();
  });

  it('ignores a response after unmount without updating canonical services', async () => {
    const pending = deferred<AddDetectedServicesResult>();
    addDetectedServicesMock.mockReturnValue(pending.promise);
    const replaceServices = vi.fn();
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result, unmount } = renderHook(() =>
      useDetectedServiceSubmission('project-a', replaceServices, mutationCoordinator),
    );
    let submission!: Promise<boolean>;
    act(() => {
      submission = result.current.submit([prepared('one')]);
    });

    unmount();
    let outcome = true;
    await act(async () => {
      pending.resolve(batchResult({ services: [service('late')] }));
      outcome = await submission;
    });

    expect(outcome).toBe(false);
    expect(replaceServices).not.toHaveBeenCalled();
    expect(errorSpy).not.toHaveBeenCalled();
  });
});

describe('useServices detected batch integration', () => {
  it.each([
    {
      label: 'partial result',
      response: batchResult({
        services: [service('partial-authoritative')],
        added: [{ stableId: 'added', service: service('partial-authoritative') }],
        skipped: [
          {
            stableId: 'skipped',
            name: 'Skipped',
            kind: 'duplicateExistingName',
            message: 'Duplicate.',
          },
        ],
      }),
    },
    {
      label: 'all-skipped result',
      response: batchResult({
        services: [service('all-skipped-authoritative')],
        skipped: [
          {
            stableId: 'skipped',
            name: 'Skipped',
            kind: 'invalidCommand',
            message: 'Invalid.',
          },
        ],
      }),
    },
  ])('replaces canonical services by reference for a current $label', async ({ response }) => {
    listServicesMock.mockResolvedValueOnce([service('initial')]);
    const pending = deferred<AddDetectedServicesResult>();
    addDetectedServicesMock.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let submission!: Promise<boolean>;

    act(() => {
      submission = result.current.detectedSubmission.submit([prepared('one')]);
    });
    expect(result.current.isMutating).toBe(true);
    await act(async () => {
      pending.resolve(response);
      await submission;
    });

    expect(result.current.services).toBe(response.services);
    expect(result.current.detectedSubmission.result).toBe(response);
    expect(result.current.detectedSubmission.status).toBe('success');
    expect(result.current.isMutating).toBe(false);
  });

  it('preserves canonical services when the batch fails globally', async () => {
    const initial = [service('initial')];
    listServicesMock.mockResolvedValueOnce(initial);
    const rejection = new Error('batch failed');
    addDetectedServicesMock.mockRejectedValue(rejection);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    let outcome = true;
    await act(async () => {
      outcome = await result.current.detectedSubmission.submit([prepared('one')]);
    });

    expect(outcome).toBe(false);
    expect(result.current.services).toBe(initial);
    expect(result.current.detectedSubmission.error).toBe(rejection);
  });

  it('clears canonical services and submission state when projectId changes', async () => {
    const servicesA = [service('from-a')];
    const pendingB = deferred<ServiceDefinition[]>();
    listServicesMock.mockResolvedValueOnce(servicesA).mockReturnValueOnce(pendingB.promise);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServices(projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.services).toBe(servicesA);

    rerender({ projectId: 'project-b' });

    expect(result.current.services).toEqual([]);
    expect(result.current.isLoading).toBe(true);
    expect(result.current.detectedSubmission).toMatchObject({
      status: 'idle',
      result: null,
      error: null,
    });
    await act(async () => {
      pendingB.resolve([service('from-b')]);
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
  });

  it('blocks manual mutation until the initial service load releases its lease', async () => {
    const pendingLoad = deferred<ServiceDefinition[]>();
    const loadedServices = [service('loaded')];
    const manualServices = [service('manual-after-load')];
    listServicesMock.mockReturnValue(pendingLoad.promise);
    addServiceMock.mockResolvedValue(manualServices);
    const { result } = renderHook(() => useServices('project-a'));

    expect(listServicesMock).toHaveBeenCalledTimes(1);
    expect(result.current.isLoading).toBe(true);
    expect(result.current.isMutating).toBe(false);
    expect(await result.current.addService(input('blocked'))).toBe(false);
    expect(addServiceMock).not.toHaveBeenCalled();
    expect(result.current.services).toEqual([]);
    expect(result.current.error).toBeNull();
    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      pendingLoad.resolve(loadedServices);
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.services).toBe(loadedServices);

    let outcome = false;
    await act(async () => {
      outcome = await result.current.addService(input('manual-after-load'));
    });
    expect(outcome).toBe(true);
    expect(addServiceMock).toHaveBeenCalledTimes(1);
    expect(result.current.services).toBe(manualServices);
  });

  it('blocks batch submission until the initial service load releases its lease', async () => {
    const pendingLoad = deferred<ServiceDefinition[]>();
    const response = batchResult({ services: [service('batch-after-load')] });
    listServicesMock.mockReturnValue(pendingLoad.promise);
    addDetectedServicesMock.mockResolvedValue(response);
    const { result } = renderHook(() => useServices('project-a'));

    expect(listServicesMock).toHaveBeenCalledTimes(1);
    expect(result.current.isMutating).toBe(false);
    expect(await result.current.detectedSubmission.submit([prepared('blocked')])).toBe(false);
    expect(addDetectedServicesMock).not.toHaveBeenCalled();
    expect(result.current.detectedSubmission).toMatchObject({
      status: 'idle',
      result: null,
      error: null,
    });
    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      pendingLoad.resolve([]);
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let outcome = false;
    await act(async () => {
      outcome = await result.current.detectedSubmission.submit([prepared('after-load')]);
    });
    expect(outcome).toBe(true);
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(1);
    expect(result.current.services).toBe(response.services);
  });

  it('releases the load lease after an error so a manual mutation can start', async () => {
    const pendingLoad = deferred<ServiceDefinition[]>();
    const manualServices = [service('after-load-error')];
    listServicesMock.mockReturnValue(pendingLoad.promise);
    addServiceMock.mockResolvedValue(manualServices);
    const { result } = renderHook(() => useServices('project-a'));

    await act(async () => {
      pendingLoad.reject(new Error('load failed'));
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.error).toBe('load failed');
    expect(result.current.isMutating).toBe(false);

    let outcome = false;
    await act(async () => {
      outcome = await result.current.addService(input('after-load-error'));
    });
    expect(outcome).toBe(true);
    expect(result.current.services).toBe(manualServices);
    expect(result.current.isLoading).toBe(false);
    expect(result.current.isMutating).toBe(false);
  });

  it('keeps load B ownership when obsolete load A settles after a project change', async () => {
    const pendingA = deferred<ServiceDefinition[]>();
    const pendingB = deferred<ServiceDefinition[]>();
    const servicesB = [service('loaded-b')];
    const afterB = [service('manual-b')];
    listServicesMock.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
    addServiceMock.mockResolvedValue(afterB);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServices(projectId),
      { initialProps: { projectId: 'project-a' } },
    );

    rerender({ projectId: 'project-b' });
    expect(listServicesMock).toHaveBeenCalledTimes(2);
    await act(async () => {
      pendingA.reject(new Error('late load-a failure'));
    });

    expect(result.current.isLoading).toBe(true);
    expect(result.current.isMutating).toBe(false);
    expect(result.current.services).toEqual([]);
    expect(result.current.error).toBeNull();
    expect(await result.current.addService(input('blocked-b'))).toBe(false);
    expect(addServiceMock).not.toHaveBeenCalled();
    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      pendingB.resolve(servicesB);
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.services).toBe(servicesB);
    expect(result.current.error).toBeNull();

    let outcome = false;
    await act(async () => {
      outcome = await result.current.addService(input('manual-b'));
    });
    expect(outcome).toBe(true);
    expect(addServiceMock).toHaveBeenCalledTimes(1);
    expect(result.current.services).toBe(afterB);
  });

  it('ignores a pending manual mutation after unmount without affecting another instance', async () => {
    const pendingManual = deferred<ServiceDefinition[]>();
    const servicesB = [service('instance-b')];
    addServiceMock.mockReturnValueOnce(pendingManual.promise).mockResolvedValueOnce(servicesB);
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const first = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(first.result.current.isLoading).toBe(false));
    let manualA!: Promise<boolean>;
    act(() => {
      manualA = first.result.current.addService(input('instance-a'));
    });
    first.unmount();

    const second = renderHook(() => useServices('project-b'));
    await waitFor(() => expect(second.result.current.isLoading).toBe(false));
    let outcomeB = false;
    await act(async () => {
      outcomeB = await second.result.current.addService(input('instance-b'));
    });
    expect(outcomeB).toBe(true);
    expect(second.result.current.services).toBe(servicesB);

    let outcomeA = true;
    await act(async () => {
      pendingManual.resolve([service('late-instance-a')]);
      outcomeA = await manualA;
    });
    expect(outcomeA).toBe(false);
    expect(second.result.current.services).toBe(servicesB);
    expect(second.result.current.error).toBeNull();
    expect(errorSpy).not.toHaveBeenCalled();
  });

  it('keeps the existing manual add mutation working', async () => {
    const manualServices = [service('manual')];
    addServiceMock.mockResolvedValue(manualServices);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    let outcome = false;
    await act(async () => {
      outcome = await result.current.addService(input('manual'));
    });

    expect(outcome).toBe(true);
    expect(result.current.services).toBe(manualServices);
    expect(addServiceMock).toHaveBeenCalledTimes(1);
  });

  it('keeps isMutating true for a pending manual mutation and blocks batch submission', async () => {
    const pending = deferred<ServiceDefinition[]>();
    const manualServices = [service('manual')];
    addServiceMock.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let manual!: Promise<boolean>;

    act(() => {
      manual = result.current.addService(input('manual'));
    });

    expect(result.current.isMutating).toBe(true);
    await expect(result.current.detectedSubmission.submit([prepared('blocked')])).resolves.toBe(
      false,
    );
    expect(addDetectedServicesMock).not.toHaveBeenCalled();
    expect(result.current.isMutating).toBe(true);

    await act(async () => {
      pending.resolve(manualServices);
      await manual;
    });
    expect(result.current.services).toBe(manualServices);
    expect(result.current.isMutating).toBe(false);
  });

  it('blocks a manual mutation while a detected batch owns the shared lease', async () => {
    const pending = deferred<AddDetectedServicesResult>();
    const response = batchResult({ services: [service('batch')] });
    addDetectedServicesMock.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let batch!: Promise<boolean>;

    act(() => {
      batch = result.current.detectedSubmission.submit([prepared('batch')]);
    });
    expect(result.current.isMutating).toBe(true);

    await expect(result.current.addService(input('blocked'))).resolves.toBe(false);
    expect(addServiceMock).not.toHaveBeenCalled();
    expect(result.current.detectedSubmission.status).toBe('submitting');

    await act(async () => {
      pending.resolve(response);
      await batch;
    });
    expect(result.current.services).toBe(response.services);
    expect(result.current.isMutating).toBe(false);
  });

  it('blocks a second same-tick manual mutation and releases the lease after completion', async () => {
    const pending = deferred<ServiceDefinition[]>();
    const firstServices = [service('first')];
    const laterServices = [service('later')];
    addServiceMock.mockReturnValue(pending.promise);
    removeServiceMock.mockResolvedValue(laterServices);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let first!: Promise<boolean>;
    let blocked!: Promise<boolean>;

    act(() => {
      first = result.current.addService(input('first'));
      blocked = result.current.updateService('service-id', input('blocked'));
    });

    await expect(blocked).resolves.toBe(false);
    expect(addServiceMock).toHaveBeenCalledTimes(1);
    expect(updateServiceMock).not.toHaveBeenCalled();
    await act(async () => {
      pending.resolve(firstServices);
      await first;
    });

    let laterOutcome = false;
    await act(async () => {
      laterOutcome = await result.current.removeService('service-id');
    });
    expect(laterOutcome).toBe(true);
    expect(removeServiceMock).toHaveBeenCalledTimes(1);
    expect(result.current.services).toBe(laterServices);
  });

  it('releases a failed manual owner so a later mutation can start', async () => {
    const laterServices = [service('later')];
    addServiceMock.mockRejectedValue(new Error('manual failed'));
    updateServiceMock.mockResolvedValue(laterServices);
    const { result } = renderHook(() => useServices('project-a'));
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    let failedOutcome = true;
    await act(async () => {
      failedOutcome = await result.current.addService(input('failed'));
    });
    expect(failedOutcome).toBe(false);
    expect(result.current.error).toBe('manual failed');
    let laterOutcome = false;
    await act(async () => {
      laterOutcome = await result.current.updateService('service-id', input('later'));
    });
    expect(laterOutcome).toBe(true);
    expect(updateServiceMock).toHaveBeenCalledTimes(1);
    expect(result.current.services).toBe(laterServices);
    expect(result.current.isMutating).toBe(false);
  });

  it('keeps batch B ownership when obsolete manual A settles after a project change', async () => {
    const pendingA = deferred<ServiceDefinition[]>();
    const pendingB = deferred<AddDetectedServicesResult>();
    const responseB = batchResult({ services: [service('from-b')] });
    const afterB = [service('after-b')];
    addServiceMock.mockReturnValueOnce(pendingA.promise).mockResolvedValueOnce(afterB);
    addDetectedServicesMock.mockReturnValue(pendingB.promise);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServices(projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let manualA!: Promise<boolean>;
    act(() => {
      manualA = result.current.addService(input('a'));
    });

    rerender({ projectId: 'project-b' });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let batchB!: Promise<boolean>;
    act(() => {
      batchB = result.current.detectedSubmission.submit([prepared('b')]);
    });
    expect(result.current.isMutating).toBe(true);

    let outcomeA = true;
    await act(async () => {
      pendingA.reject(new Error('late project-a failure'));
      outcomeA = await manualA;
    });
    expect(outcomeA).toBe(false);
    expect(result.current.error).toBeNull();
    expect(result.current.detectedSubmission.status).toBe('submitting');
    expect(result.current.isMutating).toBe(true);

    await expect(result.current.addService(input('blocked-b'))).resolves.toBe(false);
    expect(addServiceMock).toHaveBeenCalledTimes(1);
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(1);
    expect(result.current.isMutating).toBe(true);

    await act(async () => {
      pendingB.resolve(responseB);
      await batchB;
    });
    expect(result.current.services).toBe(responseB.services);
    expect(result.current.detectedSubmission.result).toBe(responseB);

    let afterBOutcome = false;
    await act(async () => {
      afterBOutcome = await result.current.addService(input('after-b'));
    });
    expect(afterBOutcome).toBe(true);
    expect(addServiceMock).toHaveBeenCalledTimes(2);
    expect(result.current.services).toBe(afterB);
  });

  it('keeps manual B ownership when obsolete batch A settles after a project change', async () => {
    const pendingA = deferred<AddDetectedServicesResult>();
    const pendingB = deferred<ServiceDefinition[]>();
    const servicesB = [service('manual-b')];
    addDetectedServicesMock.mockReturnValue(pendingA.promise);
    addServiceMock.mockReturnValue(pendingB.promise);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServices(projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let batchA!: Promise<boolean>;
    act(() => {
      batchA = result.current.detectedSubmission.submit([prepared('a')]);
    });

    rerender({ projectId: 'project-b' });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let manualB!: Promise<boolean>;
    act(() => {
      manualB = result.current.addService(input('b'));
    });

    let outcomeA = true;
    await act(async () => {
      pendingA.resolve(batchResult({ services: [service('late-a')] }));
      outcomeA = await batchA;
    });
    expect(outcomeA).toBe(false);
    expect(result.current.isMutating).toBe(true);
    await expect(result.current.detectedSubmission.submit([prepared('blocked-b')])).resolves.toBe(
      false,
    );
    expect(addDetectedServicesMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      pendingB.resolve(servicesB);
      await manualB;
    });
    expect(result.current.services).toBe(servicesB);
    expect(result.current.detectedSubmission).toMatchObject({
      status: 'idle',
      result: null,
      error: null,
    });
    expect(result.current.isMutating).toBe(false);
  });
});
