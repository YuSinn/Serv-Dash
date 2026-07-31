import { StrictMode } from 'react';
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { addService, detectProjectServices, ProjectsApiError } from './api';
import { useServiceDetection, type UseServiceDetectionResult } from './useServiceDetection';
import type { DetectionResult, DetectionWarning, ServiceSuggestion } from './types';

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return {
    ...actual,
    addService: vi.fn(),
    detectProjectServices: vi.fn(),
  };
});

const addServiceMock = vi.mocked(addService);
const detectProjectServicesMock = vi.mocked(detectProjectServices);

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

function suggestion(
  stableId: string,
  overrides: Partial<ServiceSuggestion> = {},
): ServiceSuggestion {
  return {
    stableId,
    displayName: `Service ${stableId}`,
    sourceKind: 'npmScript',
    sourcePath: 'package.json',
    workingDirectory: '.',
    command: `npm run -- ${stableId}`,
    reason: `Detected ${stableId}`,
    defaultSelected: false,
    editable: true,
    warnings: [],
    ...overrides,
  };
}

function detectionResult(
  projectId: string,
  suggestions: ServiceSuggestion[] = [],
  overrides: Partial<DetectionResult> = {},
): DetectionResult {
  return {
    projectId,
    projectRoot: `D:\\projects\\${projectId}`,
    suggestions,
    warnings: [],
    scannedDirectories: 3,
    truncated: false,
    ...overrides,
  };
}

async function scanHook(hook: { readonly current: UseServiceDetectionResult }): Promise<boolean> {
  let outcome = false;
  await act(async () => {
    outcome = await hook.current.scan();
  });
  return outcome;
}

beforeEach(() => {
  addServiceMock.mockReset();
  detectProjectServicesMock.mockReset();
});

afterEach(() => {
  expect(addServiceMock).not.toHaveBeenCalled();
  cleanup();
  vi.restoreAllMocks();
});

describe('useServiceDetection lifecycle', () => {
  it('starts idle without scanning on mount', () => {
    const { result } = renderHook(() => useServiceDetection('project-a'));

    expect(detectProjectServicesMock).not.toHaveBeenCalled();
    expect(result.current).toMatchObject({
      isOpen: false,
      status: 'idle',
      isLoading: false,
      result: null,
      drafts: [],
      error: null,
    });
    expect(result.current.selectedIds.size).toBe(0);
  });

  it('does not scan automatically under StrictMode', () => {
    renderHook(() => useServiceDetection('project-a'), { wrapper: StrictMode });

    expect(detectProjectServicesMock).not.toHaveBeenCalled();
  });

  it('opens without scanning', () => {
    const { result } = renderHook(() => useServiceDetection('project-a'));

    act(() => result.current.open());

    expect(result.current.isOpen).toBe(true);
    expect(result.current.status).toBe('idle');
    expect(detectProjectServicesMock).not.toHaveBeenCalled();
  });

  it('closes to idle and discards a completed review', async () => {
    const response = detectionResult('project-a', [suggestion('dev', { defaultSelected: true })]);
    detectProjectServicesMock.mockResolvedValue(response);
    const { result } = renderHook(() => useServiceDetection('project-a'));
    act(() => result.current.open());
    await scanHook(result);

    act(() => result.current.close());

    expect(result.current).toMatchObject({
      isOpen: false,
      status: 'idle',
      isLoading: false,
      result: null,
      drafts: [],
      error: null,
    });
    expect(result.current.selectedIds.size).toBe(0);
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(1);
  });
});

describe('useServiceDetection scanning', () => {
  it('moves from idle through loading to success and uses the exact projectId', async () => {
    const pending = deferred<DetectionResult>();
    const response = detectionResult('project-a', [suggestion('dev')]);
    detectProjectServicesMock.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useServiceDetection('project-a'));
    let scanPromise!: Promise<boolean>;

    act(() => {
      scanPromise = result.current.scan();
    });

    expect(result.current.status).toBe('loading');
    expect(result.current.isLoading).toBe(true);
    expect(result.current.error).toBeNull();
    expect(detectProjectServicesMock).toHaveBeenCalledWith('project-a');

    let outcome = false;
    await act(async () => {
      pending.resolve(response);
      outcome = await scanPromise;
    });

    expect(outcome).toBe(true);
    expect(result.current.status).toBe('success');
    expect(result.current.isLoading).toBe(false);
    expect(result.current.result).toBe(response);
  });

  it('builds drafts and initial selection with registered matches blocked', async () => {
    const matchWarning: DetectionWarning = {
      kind: 'matchesRegisteredService',
      message: 'Already registered.',
      path: 'package.json',
    };
    const response = detectionResult('project-a', [
      suggestion('default', { defaultSelected: true }),
      suggestion('manual'),
      suggestion('matched', { defaultSelected: true, warnings: [matchWarning] }),
    ]);
    detectProjectServicesMock.mockResolvedValue(response);
    const { result } = renderHook(() => useServiceDetection('project-a'));

    expect(await scanHook(result)).toBe(true);

    expect(result.current.drafts.map(({ stableId }) => stableId)).toEqual([
      'default',
      'manual',
      'matched',
    ]);
    expect([...result.current.selectedIds]).toEqual(['default']);
    expect(result.current.drafts[2]?.matchesRegisteredService).toBe(true);
  });

  it('uses empty status for a result without suggestions', async () => {
    const response = detectionResult('project-a');
    detectProjectServicesMock.mockResolvedValue(response);
    const { result } = renderHook(() => useServiceDetection('project-a'));

    expect(await scanHook(result)).toBe(true);

    expect(result.current.status).toBe('empty');
    expect(result.current.result).toBe(response);
    expect(result.current.drafts).toEqual([]);
  });

  it('preserves warnings and truncation on the complete result', async () => {
    const warnings: DetectionWarning[] = [
      { kind: 'directoryLimitReached', message: 'Limit reached.', path: null },
    ];
    const response = detectionResult('project-a', [suggestion('dev')], {
      warnings,
      truncated: true,
      scannedDirectories: 2000,
    });
    detectProjectServicesMock.mockResolvedValue(response);
    const { result } = renderHook(() => useServiceDetection('project-a'));

    await scanHook(result);

    expect(result.current.result).toBe(response);
    expect(result.current.result?.warnings).toBe(warnings);
    expect(result.current.result?.truncated).toBe(true);
    expect(result.current.result?.scannedDirectories).toBe(2000);
  });

  it('reports a ProjectsApiError message without throwing', async () => {
    detectProjectServicesMock.mockRejectedValue(
      new ProjectsApiError({ code: 'project_not_found', message: 'Project missing.' }),
    );
    const { result } = renderHook(() => useServiceDetection('missing'));

    expect(await scanHook(result)).toBe(false);

    expect(result.current.status).toBe('error');
    expect(result.current.error).toBe('Project missing.');
    expect(result.current.result).toBeNull();
  });

  it('uses the existing fallback for an unknown rejection', async () => {
    detectProjectServicesMock.mockRejectedValue({ reason: 'IPC unavailable' });
    const { result } = renderHook(() => useServiceDetection('project-a'));

    expect(await scanHook(result)).toBe(false);

    expect(result.current.status).toBe('error');
    expect(result.current.error).toBe('An unexpected error occurred.');
  });

  it('retries through scan after an error and clears that error while loading', async () => {
    const pending = deferred<DetectionResult>();
    detectProjectServicesMock
      .mockRejectedValueOnce(new Error('First scan failed.'))
      .mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useServiceDetection('project-a'));
    await scanHook(result);
    let retryPromise!: Promise<boolean>;

    act(() => {
      retryPromise = result.current.retry();
    });

    expect(result.current.status).toBe('loading');
    expect(result.current.error).toBeNull();
    let outcome = false;
    await act(async () => {
      pending.resolve(detectionResult('project-a', [suggestion('dev')]));
      outcome = await retryPromise;
    });
    expect(outcome).toBe(true);
    expect(result.current.status).toBe('success');
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(2);
  });

  it('synchronously rejects a duplicate scan while one is in flight', async () => {
    const pending = deferred<DetectionResult>();
    detectProjectServicesMock.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useServiceDetection('project-a'));
    let first!: Promise<boolean>;
    let duplicate!: Promise<boolean>;

    act(() => {
      first = result.current.scan();
      duplicate = result.current.scan();
    });

    await expect(duplicate).resolves.toBe(false);
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      pending.resolve(detectionResult('project-a', [suggestion('dev')]));
      await first;
    });
  });
});

describe('useServiceDetection stale request protection', () => {
  it('ignores a response that arrives after close', async () => {
    const pending = deferred<DetectionResult>();
    detectProjectServicesMock.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useServiceDetection('project-a'));
    act(() => result.current.open());
    let scanPromise!: Promise<boolean>;
    act(() => {
      scanPromise = result.current.scan();
    });

    act(() => result.current.close());
    let outcome = true;
    await act(async () => {
      pending.resolve(detectionResult('project-a', [suggestion('late')]));
      outcome = await scanPromise;
    });

    expect(outcome).toBe(false);
    expect(result.current.status).toBe('idle');
    expect(result.current.result).toBeNull();
    expect(result.current.isOpen).toBe(false);
  });

  it('invalidates a pending response when projectId changes', async () => {
    const pending = deferred<DetectionResult>();
    detectProjectServicesMock.mockReturnValue(pending.promise);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServiceDetection(projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    act(() => result.current.open());
    let oldScan!: Promise<boolean>;
    act(() => {
      oldScan = result.current.scan();
    });

    rerender({ projectId: 'project-b' });

    expect(result.current).toMatchObject({
      isOpen: false,
      status: 'idle',
      result: null,
      error: null,
    });
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(1);
    let outcome = true;
    await act(async () => {
      pending.resolve(detectionResult('project-a', [suggestion('late')]));
      outcome = await oldScan;
    });
    expect(outcome).toBe(false);
    expect(result.current.result).toBeNull();
  });

  it('keeps the new project result when the old request resolves last', async () => {
    const oldPending = deferred<DetectionResult>();
    const newPending = deferred<DetectionResult>();
    detectProjectServicesMock
      .mockReturnValueOnce(oldPending.promise)
      .mockReturnValueOnce(newPending.promise);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServiceDetection(projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    let oldScan!: Promise<boolean>;
    act(() => {
      oldScan = result.current.scan();
    });
    rerender({ projectId: 'project-b' });
    let newScan!: Promise<boolean>;
    act(() => {
      newScan = result.current.scan();
    });
    const newResponse = detectionResult('project-b', [suggestion('new')]);

    let newOutcome = false;
    await act(async () => {
      newPending.resolve(newResponse);
      newOutcome = await newScan;
    });
    expect(newOutcome).toBe(true);
    expect(result.current.result).toBe(newResponse);

    let oldOutcome = true;
    await act(async () => {
      oldPending.resolve(detectionResult('project-a', [suggestion('old')]));
      oldOutcome = await oldScan;
    });
    expect(oldOutcome).toBe(false);
    expect(result.current.result).toBe(newResponse);
    expect(result.current.drafts.map(({ stableId }) => stableId)).toEqual(['new']);
  });

  it('keeps the new request lock when an obsolete request settles', async () => {
    const deferredA = deferred<DetectionResult>();
    const deferredB = deferred<DetectionResult>();
    detectProjectServicesMock
      .mockReturnValueOnce(deferredA.promise)
      .mockReturnValueOnce(deferredB.promise);
    const { result, rerender } = renderHook(
      ({ projectId }: { projectId: string }) => useServiceDetection(projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    let scanA!: Promise<boolean>;
    act(() => {
      scanA = result.current.scan();
    });
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(1);
    expect(detectProjectServicesMock).toHaveBeenLastCalledWith('project-a');
    expect(result.current.status).toBe('loading');

    rerender({ projectId: 'project-b' });
    expect(result.current.status).toBe('idle');
    expect(result.current.result).toBeNull();
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(1);

    let scanB!: Promise<boolean>;
    act(() => {
      scanB = result.current.scan();
    });
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(2);
    expect(detectProjectServicesMock).toHaveBeenLastCalledWith('project-b');
    expect(result.current.status).toBe('loading');

    let outcomeA = true;
    await act(async () => {
      deferredA.resolve(detectionResult('project-a', [suggestion('from-a')]));
      outcomeA = await scanA;
    });
    expect(outcomeA).toBe(false);
    expect(result.current.status).toBe('loading');
    expect(result.current.result).toBeNull();
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(2);

    let outcomeC = true;
    await act(async () => {
      outcomeC = await result.current.scan();
    });
    expect(outcomeC).toBe(false);
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(2);
    expect(result.current.status).toBe('loading');

    const responseB = detectionResult('project-b', [suggestion('from-b')]);
    let outcomeB = false;
    await act(async () => {
      deferredB.resolve(responseB);
      outcomeB = await scanB;
    });
    expect(outcomeB).toBe(true);
    expect(result.current.status).toBe('success');
    expect(result.current.result).toBe(responseB);
    expect(result.current.result?.projectId).toBe('project-b');
    expect(result.current.drafts.map(({ stableId }) => stableId)).toEqual(['from-b']);

    const responseD = detectionResult('project-b', [suggestion('from-d')]);
    detectProjectServicesMock.mockResolvedValueOnce(responseD);
    expect(await scanHook(result)).toBe(true);
    expect(detectProjectServicesMock).toHaveBeenCalledTimes(3);
    expect(result.current.result).toBe(responseD);
  });

  it('does not update or warn after unmounting with a request pending', async () => {
    const pending = deferred<DetectionResult>();
    detectProjectServicesMock.mockReturnValue(pending.promise);
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result, unmount } = renderHook(() => useServiceDetection('project-a'));
    let scanPromise!: Promise<boolean>;
    act(() => {
      scanPromise = result.current.scan();
    });

    unmount();
    let outcome = true;
    await act(async () => {
      pending.resolve(detectionResult('project-a', [suggestion('late')]));
      outcome = await scanPromise;
    });

    expect(outcome).toBe(false);
    expect(errorSpy).not.toHaveBeenCalled();
  });
});

describe('useServiceDetection review actions', () => {
  it('toggles eligible drafts using the existing selection semantics', async () => {
    const matchWarning: DetectionWarning = {
      kind: 'matchesRegisteredService',
      message: 'Already registered.',
      path: 'package.json',
    };
    detectProjectServicesMock.mockResolvedValue(
      detectionResult('project-a', [
        suggestion('one', { defaultSelected: true }),
        suggestion('two'),
        suggestion('matched', { warnings: [matchWarning] }),
      ]),
    );
    const { result } = renderHook(() => useServiceDetection('project-a'));
    await scanHook(result);

    act(() => result.current.toggle('two'));
    expect([...result.current.selectedIds]).toEqual(['one', 'two']);
    act(() => result.current.toggle('one'));
    expect([...result.current.selectedIds]).toEqual(['two']);
    const beforeBlockedToggle = result.current.selectedIds;
    act(() => result.current.toggle('matched'));
    expect(result.current.selectedIds).toBe(beforeBlockedToggle);
  });

  it('selects all eligible drafts and excludes registered matches', async () => {
    const matchWarning: DetectionWarning = {
      kind: 'matchesRegisteredService',
      message: 'Already registered.',
      path: 'package.json',
    };
    detectProjectServicesMock.mockResolvedValue(
      detectionResult('project-a', [
        suggestion('one'),
        suggestion('matched', { warnings: [matchWarning] }),
        suggestion('two'),
      ]),
    );
    const { result } = renderHook(() => useServiceDetection('project-a'));
    await scanHook(result);

    act(() => result.current.selectAll());

    expect([...result.current.selectedIds]).toEqual(['one', 'two']);
  });

  it('clears selection through selectNone', async () => {
    detectProjectServicesMock.mockResolvedValue(
      detectionResult('project-a', [suggestion('one', { defaultSelected: true })]),
    );
    const { result } = renderHook(() => useServiceDetection('project-a'));
    await scanHook(result);

    act(() => result.current.selectNone());

    expect(result.current.selectedIds.size).toBe(0);
  });

  it('edits fields without selecting or mutating the backend result', async () => {
    const response = detectionResult('project-a', [suggestion('one')]);
    detectProjectServicesMock.mockResolvedValue(response);
    const { result } = renderHook(() => useServiceDetection('project-a'));
    await scanHook(result);

    act(() =>
      result.current.edit('one', {
        displayName: 'Edited one',
        command: 'edited command',
        workingDirectory: 'client',
      }),
    );

    expect(result.current.drafts[0]).toMatchObject({
      displayName: 'Edited one',
      command: 'edited command',
      workingDirectory: 'client',
    });
    expect(result.current.selectedIds.size).toBe(0);
    expect(response.suggestions[0]).toMatchObject({
      displayName: 'Service one',
      command: 'npm run -- one',
      workingDirectory: '.',
    });
  });

  it('resets a draft using its stableId', async () => {
    detectProjectServicesMock.mockResolvedValue(
      detectionResult('project-a', [
        suggestion('a', {
          displayName: 'Original A',
          command: 'command a',
          workingDirectory: 'directory-a',
        }),
        suggestion('b', {
          displayName: 'Original B',
          command: 'command b',
          workingDirectory: 'directory-b',
        }),
      ]),
    );
    const { result } = renderHook(() => useServiceDetection('project-a'));
    await scanHook(result);
    act(() =>
      result.current.edit('b', {
        displayName: 'Edited B',
        command: 'edited b',
        workingDirectory: 'edited-b',
      }),
    );

    act(() => result.current.resetDraft('b'));

    expect(result.current.drafts[1]).toMatchObject({
      stableId: 'b',
      displayName: 'Original B',
      command: 'command b',
      workingDirectory: 'directory-b',
    });
    expect(result.current.drafts[0]?.displayName).toBe('Original A');
  });

  it('treats every review action as a no-op before a scan', () => {
    const { result } = renderHook(() => useServiceDetection('project-a'));
    const before = result.current;

    act(() => {
      result.current.toggle('missing');
      result.current.selectAll();
      result.current.selectNone();
      result.current.edit('missing', { displayName: 'Missing' });
      result.current.resetDraft('missing');
    });

    expect(result.current).toBe(before);
    expect(detectProjectServicesMock).not.toHaveBeenCalled();
  });

  it('never persists or creates services while scanning and editing', async () => {
    detectProjectServicesMock.mockResolvedValue(
      detectionResult('project-a', [suggestion('one', { defaultSelected: true })]),
    );
    const { result } = renderHook(() => useServiceDetection('project-a'));

    await scanHook(result);
    act(() => {
      result.current.edit('one', { displayName: 'Local draft' });
      result.current.selectAll();
      result.current.close();
    });

    expect(detectProjectServicesMock).toHaveBeenCalledTimes(1);
    expect(addServiceMock).not.toHaveBeenCalled();
  });
});
