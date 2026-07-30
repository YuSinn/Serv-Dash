import type {
  DetectionResult,
  DetectionSourceKind,
  DetectionWarning,
  DetectionWarningKind,
} from './types';
import {
  validateServiceCommand,
  validateServiceName,
  validateServiceWorkingDirectory,
} from './serviceFieldValidation';

export interface DetectionDraft {
  readonly stableId: string;
  readonly sourceKind: DetectionSourceKind;
  readonly sourcePath: string;
  readonly reason: string;
  readonly warnings: readonly DetectionWarning[];
  readonly defaultSelected: boolean;
  readonly matchesRegisteredService: boolean;
  displayName: string;
  command: string;
  workingDirectory: string;
}

export interface DetectionReviewState {
  readonly originalResult: DetectionResult;
  readonly drafts: readonly DetectionDraft[];
  readonly selectedIds: ReadonlySet<string>;
}

export type DetectionDraftPatch = Readonly<
  Partial<Pick<DetectionDraft, 'displayName' | 'command' | 'workingDirectory'>>
>;

export interface DetectionDraftErrors {
  displayName?: string;
  command?: string;
  workingDirectory?: string;
}

export type DetectionWarningSeverity = 'info' | 'warning' | 'blocking';

const WARNING_SEVERITY: Record<DetectionWarningKind, DetectionWarningSeverity> = {
  invalidPackageJson: 'warning',
  emptyNpmScript: 'info',
  unsafeNpmScriptName: 'warning',
  nonUtf8Path: 'warning',
  symlinkOrJunctionSkipped: 'warning',
  permissionDenied: 'warning',
  fileDisappeared: 'warning',
  depthLimitReached: 'info',
  directoryLimitReached: 'info',
  packageJsonLimitReached: 'info',
  suggestionLimitReached: 'info',
  packageJsonTooLarge: 'warning',
  unsafeWindowsScriptName: 'warning',
  matchesRegisteredService: 'blocking',
  windowsScriptLimitReached: 'info',
};

export function hasRegisteredServiceWarning(warnings: readonly DetectionWarning[]): boolean {
  return warnings.some(({ kind }) => kind === 'matchesRegisteredService');
}

export function createDetectionReview(result: DetectionResult): DetectionReviewState {
  const drafts = result.suggestions.map((suggestion): DetectionDraft => ({
    stableId: suggestion.stableId,
    sourceKind: suggestion.sourceKind,
    sourcePath: suggestion.sourcePath,
    reason: suggestion.reason,
    warnings: suggestion.warnings,
    defaultSelected: suggestion.defaultSelected,
    matchesRegisteredService: hasRegisteredServiceWarning(suggestion.warnings),
    displayName: suggestion.displayName,
    command: suggestion.command,
    workingDirectory: suggestion.workingDirectory,
  }));
  const selectedIds = new Set(
    drafts
      .filter(({ defaultSelected, matchesRegisteredService }) => {
        return defaultSelected && !matchesRegisteredService;
      })
      .map(({ stableId }) => stableId),
  );

  return { originalResult: result, drafts, selectedIds };
}

export function selectAllEligible(state: DetectionReviewState): DetectionReviewState {
  const selectedIds = new Set(
    state.drafts
      .filter(({ matchesRegisteredService }) => !matchesRegisteredService)
      .map(({ stableId }) => stableId),
  );
  return { ...state, selectedIds };
}

export function selectNone(state: DetectionReviewState): DetectionReviewState {
  if (state.selectedIds.size === 0) {
    return state;
  }
  return { ...state, selectedIds: new Set() };
}

export function toggleSelection(
  state: DetectionReviewState,
  stableId: string,
): DetectionReviewState {
  const draft = state.drafts.find((candidate) => candidate.stableId === stableId);
  if (!draft || draft.matchesRegisteredService) {
    return state;
  }

  const selectedIds = new Set(state.selectedIds);
  if (selectedIds.has(stableId)) {
    selectedIds.delete(stableId);
  } else {
    selectedIds.add(stableId);
  }
  return { ...state, selectedIds };
}

export function editDetectionDraft(
  state: DetectionReviewState,
  stableId: string,
  patch: DetectionDraftPatch,
): DetectionReviewState {
  const index = state.drafts.findIndex((draft) => draft.stableId === stableId);
  if (index === -1) {
    return state;
  }

  const current = state.drafts[index];
  const updated: DetectionDraft = {
    ...current,
    displayName: patch.displayName ?? current.displayName,
    command: patch.command ?? current.command,
    workingDirectory: patch.workingDirectory ?? current.workingDirectory,
  };
  if (
    updated.displayName === current.displayName &&
    updated.command === current.command &&
    updated.workingDirectory === current.workingDirectory
  ) {
    return state;
  }

  return {
    ...state,
    drafts: state.drafts.map((draft, draftIndex) => (draftIndex === index ? updated : draft)),
  };
}

export function resetDetectionDraft(
  state: DetectionReviewState,
  stableId: string,
): DetectionReviewState {
  const draft = state.drafts.find((candidate) => candidate.stableId === stableId);
  if (!draft) {
    return state;
  }
  const suggestion = state.originalResult.suggestions.find(
    (candidate) => candidate.stableId === stableId,
  );
  if (!suggestion) {
    return state;
  }
  return editDetectionDraft(state, stableId, {
    displayName: suggestion.displayName,
    command: suggestion.command,
    workingDirectory: suggestion.workingDirectory,
  });
}

export function validateDetectionDraft(draft: DetectionDraft): DetectionDraftErrors {
  const errors: DetectionDraftErrors = {};
  const displayNameError = validateServiceName(draft.displayName);
  const commandError = validateServiceCommand(draft.command);
  const workingDirectoryError = validateServiceWorkingDirectory(draft.workingDirectory);

  if (displayNameError) {
    errors.displayName = displayNameError;
  }
  if (commandError) {
    errors.command = commandError;
  }
  if (workingDirectoryError) {
    errors.workingDirectory = workingDirectoryError;
  }
  return errors;
}

export function isDetectionDraftValid(draft: DetectionDraft): boolean {
  return Object.keys(validateDetectionDraft(draft)).length === 0;
}

export function getDetectionWarningSeverity(kind: DetectionWarningKind): DetectionWarningSeverity {
  return WARNING_SEVERITY[kind];
}
