import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DetectionDraft } from './detectionDraft';
import { ServiceDetectionDialog } from './ServiceDetectionDialog';
import type { DetectionResult, DetectionWarning } from './types';
import type { UseServiceDetectionResult } from './useServiceDetection';

const { addServiceMock, invokeMock } = vi.hoisted(() => ({
  addServiceMock: vi.fn(),
  invokeMock: vi.fn(),
}));

vi.mock('./api', () => ({ addService: addServiceMock }));
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

function result(overrides: Partial<DetectionResult> = {}): DetectionResult {
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
    result: result(),
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

beforeEach(() => {
  addServiceMock.mockReset();
  invokeMock.mockReset();
});

afterEach(() => {
  expect(addServiceMock).not.toHaveBeenCalled();
  expect(invokeMock).not.toHaveBeenCalled();
  cleanup();
});

describe('ServiceDetectionDialog states', () => {
  it('provides an accessible dialog title and description', () => {
    render(<ServiceDetectionDialog detection={detection()} onClose={vi.fn()} />);

    const dialog = screen.getByRole('dialog', { name: 'Detected services' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleDescription(
      'Review the detected service suggestions. Closing this dialog discards this review.',
    );
  });

  it('shows only the live loading state and Close', () => {
    render(
      <ServiceDetectionDialog
        detection={detection({ status: 'loading', isLoading: true })}
        onClose={vi.fn()}
      />,
    );

    const status = screen.getByRole('status');
    expect(status).toHaveAttribute('aria-live', 'polite');
    expect(status).toHaveTextContent('Scanning the project for services...');
    expect(screen.queryByRole('list', { name: 'Detected services' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Scan again' })).not.toBeInTheDocument();
  });

  it('shows an error alert and retries without closing', () => {
    const value = detection({ status: 'error', result: null, error: 'Detection failed.' });
    render(<ServiceDetectionDialog detection={value} onClose={vi.fn()} />);

    expect(screen.getByRole('alert')).toHaveTextContent('Detection failed.');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(value.retry).toHaveBeenCalledTimes(1);
  });

  it('shows the empty result and allows another scan', () => {
    const value = detection({ status: 'empty', drafts: [], selectedIds: new Set() });
    render(<ServiceDetectionDialog detection={value} onClose={vi.fn()} />);

    expect(screen.getByText('No services were detected in this project.')).toBeVisible();
    expect(screen.getByText('Scanned directories: 17')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Scan again' }));

    expect(value.scan).toHaveBeenCalledTimes(1);
  });
});

describe('ServiceDetectionDialog review', () => {
  it('shows selection controls, all source kinds, and preserves draft order', () => {
    const value = detection();
    render(<ServiceDetectionDialog detection={value} onClose={vi.fn()} />);

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
  });

  it('reflects selection, toggles by stableId, and blocks a registered service', () => {
    const value = detection();
    render(<ServiceDetectionDialog detection={value} onClose={vi.fn()} />);

    const selected = screen.getByLabelText('Select npm: dev');
    expect(selected).toBeChecked();
    fireEvent.click(selected);
    expect(value.toggle).toHaveBeenCalledWith('npm');

    const registered = screen.getByLabelText('Select CMD service');
    expect(registered).toBeDisabled();
    expect(screen.getByText('Already registered')).toBeVisible();
    fireEvent.click(registered);
    expect(value.toggle).not.toHaveBeenCalledWith('cmd');
  });

  it('edits each allowed field without toggling and resets the correct draft', () => {
    const value = detection();
    render(<ServiceDetectionDialog detection={value} onClose={vi.fn()} />);
    const row = screen.getAllByTestId('service-suggestion')[0];
    if (!row) {
      throw new Error('Expected the npm suggestion row.');
    }

    fireEvent.change(within(row).getByLabelText('Display name'), {
      target: { value: 'Edited name' },
    });
    fireEvent.change(within(row).getByLabelText('Command'), {
      target: { value: 'edited command' },
    });
    fireEvent.change(within(row).getByLabelText('Working directory'), {
      target: { value: 'client' },
    });
    fireEvent.click(within(row).getByRole('button', { name: 'Reset' }));

    expect(value.edit).toHaveBeenNthCalledWith(1, 'npm', { displayName: 'Edited name' });
    expect(value.edit).toHaveBeenNthCalledWith(2, 'npm', { command: 'edited command' });
    expect(value.edit).toHaveBeenNthCalledWith(3, 'npm', { workingDirectory: 'client' });
    expect(value.toggle).not.toHaveBeenCalled();
    expect(value.resetDraft).toHaveBeenCalledWith('npm');
  });

  it('shows shared validation messages with accessible field state', () => {
    const invalid = draft('invalid', {
      displayName: '',
      command: '',
      workingDirectory: 'C:\\absolute',
    });
    render(
      <ServiceDetectionDialog
        detection={detection({ drafts: [invalid], selectedIds: new Set() })}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole('textbox', { name: /Display name/ })).toHaveAttribute(
      'aria-invalid',
      'true',
    );
    expect(screen.getByRole('textbox', { name: /Command/ })).toHaveAttribute(
      'aria-invalid',
      'true',
    );
    expect(screen.getByRole('textbox', { name: /Working directory/ })).toHaveAttribute(
      'aria-invalid',
      'true',
    );
    expect(screen.getByText('Service name is required.')).toBeVisible();
    expect(screen.getByText('Command is required.')).toBeVisible();
    expect(screen.getByText('Use a path relative to the project root.')).toBeVisible();
  });

  it('shows structured warnings, truncation, scan metadata, and the unsaved note', () => {
    render(<ServiceDetectionDialog detection={detection()} onClose={vi.fn()} />);

    expect(screen.getByText('The directory limit was reached.')).toBeVisible();
    expect(screen.getByText('A source could not be inspected.')).toBeVisible();
    expect(screen.getByText('tools/private.ps1')).toBeVisible();
    expect(screen.getAllByText('info', { selector: 'strong' })).toHaveLength(1);
    expect(screen.getAllByText('warning', { selector: 'strong' })).toHaveLength(1);
    expect(screen.getAllByText('blocking', { selector: 'strong' })).toHaveLength(1);
    expect(
      screen.getByText(
        'The scan was incomplete. Some directories or files may not have been inspected.',
      ),
    ).toBeVisible();
    expect(screen.getByText('Scanned directories: 17')).toBeVisible();
    expect(screen.getByText('Selections and edits are not saved yet.')).toBeVisible();
  });

  it('runs Scan again and Close through their callbacks', () => {
    const value = detection();
    const onClose = vi.fn();
    render(<ServiceDetectionDialog detection={value} onClose={onClose} />);

    fireEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    expect(value.scan).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('ServiceDetectionDialog keyboard behavior', () => {
  it('closes on Escape and removes its listener on unmount', () => {
    const onClose = vi.fn();
    const { unmount } = render(
      <ServiceDetectionDialog detection={detection()} onClose={onClose} />,
    );

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);

    unmount();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
