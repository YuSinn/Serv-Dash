import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ServiceDetectionControl } from './ServiceDetectionControl';
import { useServiceDetection, type UseServiceDetectionResult } from './useServiceDetection';

const { addServiceMock } = vi.hoisted(() => ({ addServiceMock: vi.fn() }));

vi.mock('./api', () => ({ addService: addServiceMock }));
vi.mock('./useServiceDetection', () => ({ useServiceDetection: vi.fn() }));

const useServiceDetectionMock = vi.mocked(useServiceDetection);

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

beforeEach(() => {
  addServiceMock.mockReset();
  useServiceDetectionMock.mockReset();
});

afterEach(() => {
  expect(addServiceMock).not.toHaveBeenCalled();
  cleanup();
});

describe('ServiceDetectionControl', () => {
  it('renders the explicit action without opening or scanning on mount', () => {
    const detection = detectionState();
    useServiceDetectionMock.mockReturnValue(detection);

    render(<ServiceDetectionControl projectId="project-a" />);

    expect(useServiceDetectionMock).toHaveBeenCalledWith('project-a');
    expect(screen.getByRole('button', { name: 'Detect services' })).toBeVisible();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(detection.open).not.toHaveBeenCalled();
    expect(detection.scan).not.toHaveBeenCalled();
  });

  it('opens before starting exactly one scan on click', () => {
    const calls: string[] = [];
    const detection = detectionState({
      open: vi.fn(() => calls.push('open')),
      scan: vi.fn(async () => {
        calls.push('scan');
        return true;
      }),
    });
    useServiceDetectionMock.mockReturnValue(detection);
    render(<ServiceDetectionControl projectId="project-a" />);

    fireEvent.click(screen.getByRole('button', { name: 'Detect services' }));

    expect(calls).toEqual(['open', 'scan']);
    expect(detection.open).toHaveBeenCalledTimes(1);
    expect(detection.scan).toHaveBeenCalledTimes(1);
  });

  it('disables and labels the action while detection is loading', () => {
    useServiceDetectionMock.mockReturnValue(detectionState({ status: 'loading', isLoading: true }));

    render(<ServiceDetectionControl projectId="project-a" />);

    const button = screen.getByRole('button', { name: 'Detecting...' });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'true');
  });

  it('renders the dialog only while open and returns focus after Close', () => {
    const detection = detectionState({
      isOpen: true,
      status: 'empty',
      result: {
        projectId: 'project-a',
        projectRoot: 'D:\\projects\\project-a',
        suggestions: [],
        warnings: [],
        scannedDirectories: 1,
        truncated: false,
      },
    });
    useServiceDetectionMock.mockReturnValue(detection);
    render(<ServiceDetectionControl projectId="project-a" />);
    const trigger = screen.getByRole('button', { name: 'Detect services' });

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    expect(detection.close).toHaveBeenCalledTimes(1);
    expect(trigger).toHaveFocus();
  });
});
