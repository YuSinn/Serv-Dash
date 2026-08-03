import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DetectionDraft } from './detectionDraft';
import type { DetectionSubmissionPlan } from './detectionSubmission';
import type { ServiceDetectionPresentationPhase } from './ServiceDetectionControl';
import { ServiceDetectionDialog } from './ServiceDetectionDialog';
import type {
  AddDetectedServicesResult,
  DetectionResult,
  DetectionWarning,
  ServiceDefinition,
} from './types';
import type { UseDetectedServiceSubmissionResult } from './useDetectedServiceSubmission';
import type { UseServiceDetectionResult } from './useServiceDetection';

const { addServiceMock, invokeMock } = vi.hoisted(() => ({
  addServiceMock: vi.fn(),
  invokeMock: vi.fn(),
}));

vi.mock('./api', () => ({
  addService: addServiceMock,
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : 'An unexpected error occurred.',
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

const globalWarning: DetectionWarning = {
  kind: 'directoryLimitReached',
  message: 'The directory limit was reached.',
  path: null,
};
const rowWarning: DetectionWarning = {
  kind: 'permissionDenied',
  message: 'A source could not be inspected.',
  path: 'tools/private.ps1',
};
const registeredWarning: DetectionWarning = {
  kind: 'matchesRegisteredService',
  message: 'This command is already registered.',
  path: 'scripts/start.cmd',
};

function draft(stableId: string, overrides: Partial<DetectionDraft> = {}): DetectionDraft {
  return {
    stableId,
    sourceKind: 'npmScript',
    sourcePath: 'package.json',
    reason: `Detected ${stableId}.`,
    warnings: [],
    defaultSelected: false,
    matchesRegisteredService: false,
    displayName: `Service ${stableId}`,
    command: `npm run -- ${stableId}`,
    workingDirectory: '.',
    ...overrides,
  };
}

const drafts: readonly DetectionDraft[] = [
  draft('npm', { displayName: 'npm: dev' }),
  draft('powershell', {
    displayName: 'PowerShell service',
    sourceKind: 'powerShell',
    sourcePath: 'tools/start.ps1',
    command: 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\\start.ps1"',
    workingDirectory: 'tools',
    warnings: [rowWarning],
  }),
  draft('cmd', {
    displayName: 'CMD service',
    sourceKind: 'cmd',
    sourcePath: 'scripts/start.cmd',
    command: '".\\start.cmd"',
    workingDirectory: 'scripts',
    warnings: [registeredWarning],
    matchesRegisteredService: true,
  }),
  draft('bat', {
    displayName: 'BAT service',
    sourceKind: 'bat',
    sourcePath: 'start.bat',
    command: '".\\start.bat"',
  }),
];

function detectionResult(overrides: Partial<DetectionResult> = {}): DetectionResult {
  return {
    projectId: 'project-a',
    projectRoot: 'D:\\projects\\project-a',
    suggestions: [],
    warnings: [globalWarning],
    scannedDirectories: 17,
    truncated: true,
    ...overrides,
  };
}

function detection(overrides: Partial<UseServiceDetectionResult> = {}): UseServiceDetectionResult {
  return {
    isOpen: true,
    status: 'success',
    isLoading: false,
    result: detectionResult(),
    drafts,
    selectedIds: new Set(['npm']),
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

function batchResult(
  addedNames: readonly string[],
  skippedNames: readonly string[],
): AddDetectedServicesResult {
  const added = addedNames.map((name) => ({ stableId: `added-${name}`, service: service(name) }));
  return {
    added,
    skipped: skippedNames.map((name) => ({
      stableId: `skipped-${name}`,
      name,
      kind: 'duplicateExistingName',
      message: `The service ${name} already exists.`,
    })),
    services: added.map((item) => item.service),
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

const defaultPlan: DetectionSubmissionPlan = {
  eligible: [
    {
      stableId: 'npm',
      input: {
        name: 'npm: dev',
        workingDirectory: '.',
        command: 'npm run -- dev',
        expectedPort: null,
        localUrl: null,
      },
    },
  ],
  skipped: [],
  missingSelectedIds: [],
};

interface DialogOptions {
  detection?: UseServiceDetectionResult;
  submission?: UseDetectedServiceSubmissionResult;
  phase?: ServiceDetectionPresentationPhase;
  plan?: DetectionSubmissionPlan;
  busy?: boolean;
  onClose?: () => void;
  onContinue?: () => void;
  onBack?: () => void;
  onConfirm?: () => void;
  onScanAgain?: () => void;
}

function renderDialog(options: DialogOptions = {}) {
  const callbacks = {
    onClose: options.onClose ?? vi.fn(),
    onContinue: options.onContinue ?? vi.fn(),
    onBack: options.onBack ?? vi.fn(),
    onConfirm: options.onConfirm ?? vi.fn(),
    onScanAgain: options.onScanAgain ?? vi.fn(),
  };
  const view = render(
    <ServiceDetectionDialog
      detection={options.detection ?? detection()}
      submission={options.submission ?? submission()}
      phase={options.phase ?? 'review'}
      plan={options.plan ?? defaultPlan}
      busy={options.busy ?? false}
      {...callbacks}
    />,
  );
  return { ...view, ...callbacks };
}

beforeEach(() => {
  addServiceMock.mockReset();
  invokeMock.mockReset();
});

afterEach(() => {
  expect(addServiceMock).not.toHaveBeenCalled();
  expect(invokeMock).not.toHaveBeenCalled();
  cleanup();
});

describe('ServiceDetectionDialog detection states', () => {
  it('keeps the accessible dialog shell and loading state', () => {
    renderDialog({ detection: detection({ status: 'loading', isLoading: true }) });

    const dialog = screen.getByRole('dialog', { name: 'Detected services' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleDescription(
      'Review the detected service suggestions. Closing this dialog discards this review.',
    );
    expect(screen.getByRole('status')).toHaveTextContent('Scanning the project for services...');
    expect(screen.queryByRole('list', { name: 'Detected services' })).not.toBeInTheDocument();
  });

  it('retries detection errors and scans again after an empty result', () => {
    const errorDetection = detection({ status: 'error', result: null, error: 'Detection failed.' });
    const first = renderDialog({ detection: errorDetection });
    expect(screen.getByRole('alert')).toHaveTextContent('Detection failed.');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(errorDetection.retry).toHaveBeenCalledTimes(1);
    first.unmount();

    const onScanAgain = vi.fn();
    renderDialog({
      detection: detection({ status: 'empty', drafts: [], selectedIds: new Set() }),
      onScanAgain,
    });
    expect(screen.getByText('No services were detected in this project.')).toBeVisible();
    expect(screen.getByText('Scanned directories: 17')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    expect(onScanAgain).toHaveBeenCalledTimes(1);
  });
});

describe('ServiceDetectionDialog review', () => {
  it('preserves review, selection, editing, reset, and all source kinds', () => {
    const value = detection();
    renderDialog({ detection: value });

    expect(screen.getByText('1 of 4 selected')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Select all' }));
    fireEvent.click(screen.getByRole('button', { name: 'Select none' }));
    expect(value.selectAll).toHaveBeenCalledTimes(1);
    expect(value.selectNone).toHaveBeenCalledTimes(1);

    const rows = screen.getAllByTestId('service-suggestion');
    expect(
      rows.map((row) => (within(row).getByLabelText('Display name') as HTMLInputElement).value),
    ).toEqual(['npm: dev', 'PowerShell service', 'CMD service', 'BAT service']);
    for (const label of ['npm', 'PowerShell', 'CMD', 'BAT']) {
      expect(screen.getByText(label, { selector: '.detection-source-badge' })).toBeVisible();
    }

    const selected = screen.getByLabelText('Select npm: dev');
    expect(selected).toBeChecked();
    fireEvent.click(selected);
    expect(value.toggle).toHaveBeenCalledWith('npm');
    expect(screen.getByLabelText('Select CMD service')).toBeDisabled();
    expect(screen.getByText('Already registered')).toBeVisible();

    const firstRow = rows[0];
    if (!firstRow) {
      throw new Error('Expected the npm suggestion row.');
    }
    fireEvent.change(within(firstRow).getByLabelText('Display name'), {
      target: { value: 'Edited name' },
    });
    fireEvent.change(within(firstRow).getByLabelText('Command'), {
      target: { value: 'edited command' },
    });
    fireEvent.change(within(firstRow).getByLabelText('Working directory'), {
      target: { value: 'client' },
    });
    fireEvent.click(within(firstRow).getByRole('button', { name: 'Reset' }));
    expect(value.edit).toHaveBeenNthCalledWith(1, 'npm', { displayName: 'Edited name' });
    expect(value.edit).toHaveBeenNthCalledWith(2, 'npm', { command: 'edited command' });
    expect(value.edit).toHaveBeenNthCalledWith(3, 'npm', { workingDirectory: 'client' });
    expect(value.resetDraft).toHaveBeenCalledWith('npm');
  });

  it('retains validation, warnings, truncation, and the unsaved note', () => {
    const invalid = draft('invalid', {
      displayName: '',
      command: '',
      workingDirectory: 'C:\\absolute',
      warnings: [rowWarning],
    });
    renderDialog({ detection: detection({ drafts: [invalid], selectedIds: new Set() }) });

    for (const name of [/Display name/, /Command/, /Working directory/]) {
      expect(screen.getByRole('textbox', { name })).toHaveAttribute('aria-invalid', 'true');
    }
    expect(screen.getByText('Service name is required.')).toBeVisible();
    expect(screen.getByText('Command is required.')).toBeVisible();
    expect(screen.getByText('Use a path relative to the project root.')).toBeVisible();
    expect(screen.getByText('The directory limit was reached.')).toBeVisible();
    expect(screen.getByText('A source could not be inspected.')).toBeVisible();
    expect(
      screen.getByText(
        'The scan was incomplete. Some directories or files may not have been inspected.',
      ),
    ).toBeVisible();
    expect(screen.getByText('Selections and edits are not saved yet.')).toBeVisible();
  });

  it('disables Continue without a selection and delegates it without submitting', () => {
    const onContinue = vi.fn();
    const submissionValue = submission();
    const { rerender } = renderDialog({
      detection: detection({ selectedIds: new Set() }),
      submission: submissionValue,
      onContinue,
    });
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();

    rerender(
      <ServiceDetectionDialog
        detection={detection()}
        submission={submissionValue}
        phase="review"
        plan={defaultPlan}
        busy={false}
        onClose={vi.fn()}
        onContinue={onContinue}
        onBack={vi.fn()}
        onConfirm={vi.fn()}
        onScanAgain={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect(onContinue).toHaveBeenCalledTimes(1);
    expect(submissionValue.submit).not.toHaveBeenCalled();
  });
});

describe('ServiceDetectionDialog confirmation and submission', () => {
  it('shows the current plan counts and delegates Back, Confirm, and Close', () => {
    const plan: DetectionSubmissionPlan = {
      eligible: defaultPlan.eligible,
      skipped: [{ stableId: 'invalid', displayName: 'Invalid', kind: 'invalidDraft', errors: {} }],
      missingSelectedIds: ['missing-a', 'missing-b'],
    };
    const onBack = vi.fn();
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    renderDialog({ phase: 'confirm', plan, onBack, onConfirm, onClose });

    expect(screen.getByText('1 services eligible to add.')).toBeVisible();
    expect(screen.getByText('1 selected services omitted locally.')).toBeVisible();
    expect(screen.getByText('2 selected services no longer present.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add 1 service' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onBack).toHaveBeenCalledTimes(1);
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('does not allow an empty plan to submit', () => {
    const onConfirm = vi.fn();
    renderDialog({
      phase: 'confirm',
      plan: { eligible: [], skipped: defaultPlan.skipped, missingSelectedIds: [] },
      onConfirm,
    });
    const button = screen.getByRole('button', { name: 'Add 0 services' });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('blocks close, back, editing, selection, rescan, and Escape while busy', () => {
    const onClose = vi.fn();
    const onScanAgain = vi.fn();
    renderDialog({ busy: true, onClose, onScanAgain });

    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Scan again' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Close' })).toBeDisabled();
    expect(screen.getByLabelText('Select npm: dev')).toBeDisabled();
    for (const input of screen.getAllByRole('textbox', { name: /Display name/ })) {
      expect(input).toBeDisabled();
    }
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
    expect(onScanAgain).not.toHaveBeenCalled();

    cleanup();
    renderDialog({
      phase: 'submitted',
      busy: true,
      submission: submission({ status: 'submitting' }),
      onClose,
    });
    expect(screen.getByRole('status')).toHaveTextContent('Adding detected services...');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });

  it.each([
    ['total', batchResult(['One'], [])],
    ['partial', batchResult(['One'], ['Two'])],
    ['all-skipped', batchResult([], ['Two'])],
  ])('presents %s backend outcomes as success', (_label, response) => {
    renderDialog({
      phase: 'submitted',
      submission: submission({ status: 'success', result: response }),
    });
    expect(screen.getByRole('heading', { name: 'Service import complete' })).toBeVisible();
    expect(screen.getByText(`${response.added.length} services added.`)).toBeVisible();
    expect(
      screen.getByText(`${response.skipped.length} services skipped by the server.`),
    ).toBeVisible();
  });

  it('keeps a global error separate from skipped results', () => {
    renderDialog({
      phase: 'submitted',
      submission: submission({ status: 'error', error: new Error('Batch persistence failed.') }),
    });
    expect(screen.getByRole('alert')).toHaveTextContent('Batch persistence failed.');
    expect(screen.queryByLabelText('Server-skipped services')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back to review' })).toBeVisible();
  });

  it('renders backend names and messages as escaped React text', () => {
    const unsafe = '<script>alert("x")</script>';
    const response = batchResult([unsafe], [unsafe]);
    const skipped = response.skipped[0];
    if (!skipped) {
      throw new Error('Expected a skipped service fixture.');
    }
    skipped.message = unsafe;
    renderDialog({
      phase: 'submitted',
      submission: submission({ status: 'success', result: response }),
    });

    expect(screen.getAllByText(unsafe).length).toBeGreaterThan(0);
    expect(document.querySelector('script')).toBeNull();
  });
});

describe('ServiceDetectionDialog keyboard behavior', () => {
  it('closes on Escape when idle and removes its listener on unmount', () => {
    const onClose = vi.fn();
    const { unmount } = renderDialog({ onClose });

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);

    unmount();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
