import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DetectionDraft } from './detectionDraft';
import { ServiceDetectionControl } from './ServiceDetectionControl';
import type { AddDetectedServicesResult, DetectionResult, ServiceDefinition } from './types';
import type { UseDetectedServiceSubmissionResult } from './useDetectedServiceSubmission';
import { useServiceDetection, type UseServiceDetectionResult } from './useServiceDetection';

const { addServiceMock, mockDialogProbe } = vi.hoisted(() => ({
  addServiceMock: vi.fn(),
  mockDialogProbe: {
    enabled: false,
    confirmHandler: null as (() => void) | null,
  },
}));

vi.mock('./api', () => ({ addService: addServiceMock }));
vi.mock('./useServiceDetection', () => ({ useServiceDetection: vi.fn() }));
vi.mock('./ServiceDetectionDialog', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./ServiceDetectionDialog')>();
  const ActualDialog = actual.ServiceDetectionDialog;

  return {
    ...actual,
    ServiceDetectionDialog(props: ComponentProps<typeof ActualDialog>) {
      if (!mockDialogProbe.enabled) {
        return <ActualDialog {...props} />;
      }
      if (props.phase === 'confirm') {
        mockDialogProbe.confirmHandler = props.onConfirm;
        return (
          <button
            type="button"
            onClick={() => {
              props.onConfirm();
              props.onConfirm();
            }}
          >
            Confirm twice synchronously
          </button>
        );
      }
      if (props.phase === 'submitted') {
        return (
          <>
            <button type="button" onClick={() => mockDialogProbe.confirmHandler?.()}>
              Invoke captured confirmation
            </button>
            <button type="button" onClick={props.onBack}>
              Back to review
            </button>
          </>
        );
      }
      return (
        <button type="button" onClick={props.onContinue}>
          Continue
        </button>
      );
    },
  };
});

const useServiceDetectionMock = vi.mocked(useServiceDetection);

function draft(stableId: string, overrides: Partial<DetectionDraft> = {}): DetectionDraft {
  return {
    stableId,
    sourceKind: 'npmScript',
    sourcePath: 'package.json',
    reason: `Detected ${stableId}.`,
    warnings: [],
    defaultSelected: true,
    matchesRegisteredService: false,
    displayName: `Service ${stableId}`,
    command: `npm run -- ${stableId}`,
    workingDirectory: '.',
    ...overrides,
  };
}

function result(projectId = 'project-a'): DetectionResult {
  return {
    projectId,
    projectRoot: `D:\\projects\\${projectId}`,
    suggestions: [],
    warnings: [],
    scannedDirectories: 1,
    truncated: false,
  };
}

function detectionState(
  overrides: Partial<UseServiceDetectionResult> = {},
): UseServiceDetectionResult {
  return {
    isOpen: false,
    status: 'idle',
    isLoading: false,
    result: null,
    drafts: [],
    selectedIds: new Set(),
    error: null,
    open: vi.fn(),
    close: vi.fn(),
    scan: vi.fn().mockResolvedValue(true),
    retry: vi.fn().mockResolvedValue(true),
    toggle: vi.fn(),
    selectAll: vi.fn(),
    selectNone: vi.fn(),
    edit: vi.fn(),
    resetDraft: vi.fn(),
    ...overrides,
  };
}

function submissionState(
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

interface Deferred<T> {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function service(name: string): ServiceDefinition {
  return {
    id: `service-${name}`,
    name,
    workingDirectory: '.',
    command: `run ${name}`,
    expectedPort: null,
    localUrl: null,
    createdAt: '2026-07-31T00:00:00Z',
    updatedAt: '2026-07-31T00:00:00Z',
  };
}

beforeEach(() => {
  addServiceMock.mockReset();
  useServiceDetectionMock.mockReset();
  mockDialogProbe.enabled = false;
  mockDialogProbe.confirmHandler = null;
});

afterEach(() => {
  expect(addServiceMock).not.toHaveBeenCalled();
  cleanup();
});

describe('ServiceDetectionControl', () => {
  it('renders the explicit action without opening, scanning, or submitting on mount', () => {
    const detection = detectionState();
    const submission = submissionState();
    useServiceDetectionMock.mockReturnValue(detection);

    render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );

    expect(useServiceDetectionMock).toHaveBeenCalledWith('project-a');
    expect(screen.getByRole('button', { name: 'Detect services' })).toBeVisible();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(detection.open).not.toHaveBeenCalled();
    expect(detection.scan).not.toHaveBeenCalled();
    expect(submission.submit).not.toHaveBeenCalled();
  });

  it('resets presentation, opens, and starts exactly one explicit scan', () => {
    const calls: string[] = [];
    const detection = detectionState({
      open: vi.fn(() => calls.push('open')),
      scan: vi.fn(async () => {
        calls.push('scan');
        return true;
      }),
    });
    const submission = submissionState({ reset: vi.fn(() => calls.push('reset')) });
    useServiceDetectionMock.mockReturnValue(detection);
    render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Detect services' }));

    expect(calls).toEqual(['reset', 'open', 'scan']);
    expect(detection.scan).toHaveBeenCalledTimes(1);
  });

  it('disables detection while scanning or while canonical services are busy', () => {
    useServiceDetectionMock.mockReturnValue(detectionState({ status: 'loading', isLoading: true }));
    const submission = submissionState();
    const { rerender } = render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );

    const loadingButton = screen.getByRole('button', { name: 'Detecting...' });
    expect(loadingButton).toBeDisabled();
    expect(loadingButton).toHaveAttribute('aria-busy', 'true');

    useServiceDetectionMock.mockReturnValue(detectionState());
    rerender(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy
      />,
    );
    const busyButton = screen.getByRole('button', { name: 'Detect services' });
    expect(busyButton).toBeDisabled();
    fireEvent.click(busyButton);
    expect(submission.reset).not.toHaveBeenCalled();
  });

  it('closes, resets presentation, and returns focus after review', () => {
    const submission = submissionState();
    const detection = detectionState({ isOpen: true, status: 'empty', result: result() });
    useServiceDetectionMock.mockReturnValue(detection);
    render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );
    const trigger = screen.getByRole('button', { name: 'Detect services' });

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    expect(submission.reset).toHaveBeenCalledTimes(1);
    expect(detection.close).toHaveBeenCalledTimes(1);
    expect(trigger).toHaveFocus();
  });

  it('builds a live confirmation plan and submits only eligible selected services', () => {
    const eligible = draft('eligible');
    const invalid = draft('invalid', { command: '' });
    const submission = submissionState();
    const detection = detectionState({
      isOpen: true,
      status: 'success',
      result: result(),
      drafts: [eligible, invalid],
      selectedIds: new Set(['eligible', 'invalid', 'missing']),
    });
    useServiceDetectionMock.mockReturnValue(detection);
    render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));

    expect(screen.getByText('1 services eligible to add.')).toBeVisible();
    expect(screen.getByText('1 selected services omitted locally.')).toBeVisible();
    expect(screen.getByText('1 selected services no longer present.')).toBeVisible();
    expect(submission.submit).not.toHaveBeenCalled();

    const confirm = screen.getByRole('button', { name: 'Add 1 service' });
    fireEvent.click(confirm);

    expect(submission.submit).toHaveBeenCalledTimes(1);
    expect(submission.submit).toHaveBeenCalledWith([
      {
        stableId: 'eligible',
        input: {
          name: 'Service eligible',
          workingDirectory: '.',
          command: 'npm run -- eligible',
          expectedPort: null,
          localUrl: null,
        },
      },
    ]);
  });

  it('guards synchronous confirmations and preserves each owner by identity', async () => {
    mockDialogProbe.enabled = true;
    const first = deferred<boolean>();
    const second = deferred<boolean>();
    const third = deferred<boolean>();
    const fourth = deferred<boolean>();
    const submit = vi
      .fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
      .mockImplementationOnce(() => third.promise)
      .mockImplementationOnce(() => fourth.promise);
    const submission = submissionState({ submit });
    const selectedDraft = draft('eligible');
    useServiceDetectionMock.mockReturnValue(
      detectionState({
        isOpen: true,
        status: 'success',
        result: result(),
        drafts: [selectedDraft],
        selectedIds: new Set(['eligible']),
      }),
    );
    const { rerender } = render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );
    const expectedPayload = [
      {
        stableId: 'eligible',
        input: {
          name: 'Service eligible',
          workingDirectory: '.',
          command: 'npm run -- eligible',
          expectedPort: null,
          localUrl: null,
        },
      },
    ];

    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    // One connected child event invokes the same real prop twice before React can rerender.
    fireEvent.click(screen.getByRole('button', { name: 'Confirm twice synchronously' }));
    expect(submit).toHaveBeenCalledTimes(1);
    expect(submit).toHaveBeenNthCalledWith(1, expectedPayload);

    await act(async () => {
      first.resolve(true);
      await first.promise;
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Back to review' }));
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm twice synchronously' }));
    expect(submit).toHaveBeenCalledTimes(2);
    expect(submit).toHaveBeenNthCalledWith(2, expectedPayload);

    rerender(
      <ServiceDetectionControl
        projectId="project-b"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm twice synchronously' }));
    expect(submit).toHaveBeenCalledTimes(3);
    expect(submit).toHaveBeenNthCalledWith(3, expectedPayload);

    await act(async () => {
      second.resolve(true);
      await second.promise;
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Invoke captured confirmation' }));
    expect(submit).toHaveBeenCalledTimes(3);

    await act(async () => {
      third.resolve(true);
      await third.promise;
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Back to review' }));
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm twice synchronously' }));
    expect(submit).toHaveBeenCalledTimes(4);
    expect(submit).toHaveBeenNthCalledWith(4, expectedPayload);

    await act(async () => {
      fourth.resolve(true);
      await fourth.promise;
    });
  });

  it('keeps confirmation derived from the current canonical service list', () => {
    const selectedDraft = draft('eligible');
    const submission = submissionState();
    const detection = detectionState({
      isOpen: true,
      status: 'success',
      result: result(),
      drafts: [selectedDraft],
      selectedIds: new Set(['eligible']),
    });
    useServiceDetectionMock.mockReturnValue(detection);
    const { rerender } = render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect(screen.getByText('1 services eligible to add.')).toBeVisible();

    rerender(
      <ServiceDetectionControl
        projectId="project-a"
        services={[service('Service eligible')]}
        submission={submission}
        isBusy={false}
      />,
    );

    expect(screen.getByText('0 services eligible to add.')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Add 0 services' })).toBeDisabled();
  });

  it('clears confirmation presentation when the project changes', () => {
    const selectedDraft = draft('eligible');
    const submission = submissionState();
    useServiceDetectionMock.mockReturnValue(
      detectionState({
        isOpen: true,
        status: 'success',
        result: result(),
        drafts: [selectedDraft],
        selectedIds: new Set(['eligible']),
      }),
    );
    const { rerender } = render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect(screen.getByRole('heading', { name: 'Confirm detected services' })).toBeVisible();

    rerender(
      <ServiceDetectionControl
        projectId="project-b"
        services={[]}
        submission={submission}
        isBusy={false}
      />,
    );

    expect(
      screen.queryByRole('heading', { name: 'Confirm detected services' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Continue' })).toBeVisible();
  });

  it('shows the complete batch result without creating a parallel service list', () => {
    const selectedDraft = draft('eligible');
    const backendService = service('Canonical backend service');
    const batchResult: AddDetectedServicesResult = {
      added: [{ stableId: 'eligible', service: backendService }],
      skipped: [],
      services: [backendService],
    };
    const pendingSubmission = submissionState();
    const detection = detectionState({
      isOpen: true,
      status: 'success',
      result: result(),
      drafts: [selectedDraft],
      selectedIds: new Set(['eligible']),
    });
    useServiceDetectionMock.mockReturnValue(detection);
    const { rerender } = render(
      <ServiceDetectionControl
        projectId="project-a"
        services={[]}
        submission={pendingSubmission}
        isBusy={false}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add 1 service' }));

    const completedSubmission = submissionState({ status: 'success', result: batchResult });
    rerender(
      <ServiceDetectionControl
        projectId="project-a"
        services={batchResult.services}
        submission={completedSubmission}
        isBusy={false}
      />,
    );

    expect(screen.getByRole('heading', { name: 'Service import complete' })).toBeVisible();
    expect(screen.getByText('Canonical backend service')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(completedSubmission.reset).toHaveBeenCalledTimes(1);
    expect(detection.close).toHaveBeenCalledTimes(1);
  });
});
