import { describe, expect, it } from 'vitest';
import {
  createDetectionReview,
  editDetectionDraft,
  getDetectionWarningSeverity,
  hasRegisteredServiceWarning,
  isDetectionDraftValid,
  resetDetectionDraft,
  selectAllEligible,
  selectNone,
  toggleSelection,
  validateDetectionDraft,
  type DetectionDraftPatch,
  type DetectionWarningSeverity,
} from './detectionDraft';
import {
  validateServiceCommand,
  validateServiceName,
  validateServiceWorkingDirectory,
} from './serviceFieldValidation';
import type {
  DetectionResult,
  DetectionSourceKind,
  DetectionWarning,
  DetectionWarningKind,
  ServiceSuggestion,
} from './types';

function makeWarning(kind: DetectionWarningKind, message = `Warning: ${kind}`): DetectionWarning {
  return { kind, message, path: 'package.json' };
}

function makeSuggestion(
  stableId: string,
  sourceKind: DetectionSourceKind = 'npmScript',
  overrides: Partial<ServiceSuggestion> = {},
): ServiceSuggestion {
  return {
    stableId,
    displayName: `Service ${stableId}`,
    sourceKind,
    sourcePath: sourceKind === 'npmScript' ? 'package.json' : `scripts/${stableId}.cmd`,
    workingDirectory: '.',
    command: `run ${stableId}`,
    reason: `Detected ${stableId}`,
    defaultSelected: false,
    editable: true,
    warnings: [],
    ...overrides,
  };
}

function makeResult(suggestions: ServiceSuggestion[]): DetectionResult {
  return {
    projectId: 'project-1',
    projectRoot: 'D:\\projects\\one',
    suggestions,
    warnings: [],
    scannedDirectories: 4,
    truncated: false,
  };
}

const sourceKinds = [
  'npmScript',
  'powerShell',
  'cmd',
  'bat',
] as const satisfies readonly DetectionSourceKind[];

describe('createDetectionReview', () => {
  it('builds all source kinds in suggestion order and preserves the result reference', () => {
    const result = makeResult(
      sourceKinds.map((sourceKind, index) => makeSuggestion(`id-${index}`, sourceKind)),
    );

    const state = createDetectionReview(result);

    expect(state.originalResult).toBe(result);
    expect(state.drafts.map(({ sourceKind }) => sourceKind)).toEqual(sourceKinds);
    expect(state.drafts.map(({ stableId }) => stableId)).toEqual(['id-0', 'id-1', 'id-2', 'id-3']);
  });

  it('copies suggestion metadata and editable values into a separate draft', () => {
    const warnings = [makeWarning('emptyNpmScript')];
    const suggestion = makeSuggestion('npm', 'npmScript', {
      displayName: 'npm: dev',
      sourcePath: 'client/package.json',
      workingDirectory: 'client',
      command: 'npm run -- dev',
      reason: 'npm script declared in package.json',
      defaultSelected: true,
      warnings,
    });

    const draft = createDetectionReview(makeResult([suggestion])).drafts[0];

    expect(draft).toEqual({
      stableId: 'npm',
      sourceKind: 'npmScript',
      sourcePath: 'client/package.json',
      reason: 'npm script declared in package.json',
      warnings,
      defaultSelected: true,
      matchesRegisteredService: false,
      displayName: 'npm: dev',
      command: 'npm run -- dev',
      workingDirectory: 'client',
    });
    expect(draft).not.toBe(suggestion);
    expect(draft?.warnings).toBe(warnings);
  });

  it('initially selects only defaults without a registered-service match', () => {
    const matched = makeSuggestion('matched', 'npmScript', {
      defaultSelected: true,
      warnings: [makeWarning('matchesRegisteredService')],
    });
    const result = makeResult([
      makeSuggestion('default', 'npmScript', { defaultSelected: true }),
      makeSuggestion('manual'),
      matched,
    ]);

    const state = createDetectionReview(result);

    expect([...state.selectedIds]).toEqual(['default']);
    expect(state.drafts[2]?.matchesRegisteredService).toBe(true);
  });

  it('creates fresh drafts and selection for a conceptual rescan', () => {
    const stateA = editDetectionDraft(
      createDetectionReview(
        makeResult([makeSuggestion('a', 'npmScript', { defaultSelected: true })]),
      ),
      'a',
      { displayName: 'Edited A' },
    );
    const resultB = makeResult([makeSuggestion('b', 'bat')]);

    const stateB = createDetectionReview(resultB);

    expect(stateA.drafts[0]?.displayName).toBe('Edited A');
    expect(stateB.originalResult).toBe(resultB);
    expect(stateB.drafts.map(({ stableId }) => stableId)).toEqual(['b']);
    expect(stateB.selectedIds.size).toBe(0);
  });
});

describe('selection', () => {
  it('does not toggle a registered-service match', () => {
    const state = createDetectionReview(
      makeResult([
        makeSuggestion('matched', 'powerShell', {
          defaultSelected: true,
          warnings: [makeWarning('matchesRegisteredService')],
        }),
      ]),
    );

    expect(toggleSelection(state, 'matched')).toBe(state);
    expect(state.selectedIds.size).toBe(0);
  });

  it('selects all eligible drafts and preserves drafts and result', () => {
    const state = createDetectionReview(
      makeResult([
        makeSuggestion('one'),
        makeSuggestion('matched', 'cmd', {
          warnings: [makeWarning('matchesRegisteredService')],
        }),
        makeSuggestion('two', 'bat'),
      ]),
    );

    const selected = selectAllEligible(state);

    expect([...selected.selectedIds]).toEqual(['one', 'two']);
    expect(selected.drafts).toBe(state.drafts);
    expect(selected.originalResult).toBe(state.originalResult);
  });

  it('selects and deselects an eligible draft without mutating the original Set', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one')]));

    const selected = toggleSelection(state, 'one');
    const deselected = toggleSelection(selected, 'one');

    expect(state.selectedIds.size).toBe(0);
    expect(selected.selectedIds).not.toBe(state.selectedIds);
    expect([...selected.selectedIds]).toEqual(['one']);
    expect(deselected.selectedIds.size).toBe(0);
  });

  it('selectNone clears selection without changing drafts', () => {
    const state = createDetectionReview(
      makeResult([makeSuggestion('one', 'npmScript', { defaultSelected: true })]),
    );

    const cleared = selectNone(state);

    expect(cleared.selectedIds.size).toBe(0);
    expect(cleared.drafts).toBe(state.drafts);
    expect(cleared.originalResult).toBe(state.originalResult);
  });

  it('treats an unknown stable ID as a no-op', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one')]));

    expect(toggleSelection(state, 'missing')).toBe(state);
    expect(editDetectionDraft(state, 'missing', { displayName: 'Missing' })).toBe(state);
    expect(resetDetectionDraft(state, 'missing')).toBe(state);
  });
});

describe('draft editing and reset', () => {
  it('edits only displayName and does not select the draft', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one')]));
    const patch: DetectionDraftPatch = { displayName: 'Renamed service' };

    const edited = editDetectionDraft(state, 'one', patch);

    expect(edited.drafts[0]).toMatchObject({
      displayName: 'Renamed service',
      command: 'run one',
      workingDirectory: '.',
      stableId: 'one',
    });
    expect(edited.selectedIds.size).toBe(0);

    if (false) {
      // @ts-expect-error DetectionDraftPatch cannot edit immutable metadata.
      editDetectionDraft(state, 'one', { reason: 'Changed metadata' });
    }
  });

  it('edits command', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one')]));
    const edited = editDetectionDraft(state, 'one', { command: 'npm run -- dev' });

    expect(edited.drafts[0]?.command).toBe('npm run -- dev');
    expect(edited.drafts[0]?.displayName).toBe('Service one');
  });

  it('edits workingDirectory', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one')]));
    const edited = editDetectionDraft(state, 'one', { workingDirectory: 'client' });

    expect(edited.drafts[0]?.workingDirectory).toBe('client');
    expect(edited.drafts[0]?.command).toBe('run one');
  });

  it('does not mutate prior state, drafts, or unaffected drafts', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one'), makeSuggestion('two')]));
    const originalDraft = state.drafts[0];
    const otherDraft = state.drafts[1];

    const edited = editDetectionDraft(state, 'one', { displayName: 'Edited' });

    expect(state.drafts[0]).toBe(originalDraft);
    expect(state.drafts[0]?.displayName).toBe('Service one');
    expect(edited.drafts).not.toBe(state.drafts);
    expect(edited.drafts[0]).not.toBe(originalDraft);
    expect(edited.drafts[1]).toBe(otherDraft);
  });

  it('returns the same state when a patch changes no values', () => {
    const state = createDetectionReview(makeResult([makeSuggestion('one')]));

    expect(editDetectionDraft(state, 'one', { displayName: 'Service one' })).toBe(state);
  });

  it('resets editable values and preserves selection and metadata', () => {
    const state = toggleSelection(
      createDetectionReview(makeResult([makeSuggestion('one')])),
      'one',
    );
    const edited = editDetectionDraft(state, 'one', {
      displayName: 'Edited',
      command: 'edited command',
      workingDirectory: 'edited',
    });

    const reset = resetDetectionDraft(edited, 'one');

    expect(reset.drafts[0]).toMatchObject({
      stableId: 'one',
      sourceKind: 'npmScript',
      displayName: 'Service one',
      command: 'run one',
      workingDirectory: '.',
    });
    expect([...reset.selectedIds]).toEqual(['one']);
  });

  it('resets a reordered draft from the suggestion with the same stable ID', () => {
    const suggestionA = makeSuggestion('a', 'npmScript', {
      displayName: 'Original A',
      command: 'command a',
      workingDirectory: 'directory-a',
    });
    const suggestionB = makeSuggestion('b', 'cmd', {
      displayName: 'Original B',
      command: 'command b',
      workingDirectory: 'directory-b',
      defaultSelected: true,
    });
    const original = createDetectionReview(makeResult([suggestionA, suggestionB]));
    const reordered = {
      ...original,
      drafts: [original.drafts[1]!, original.drafts[0]!],
    };
    const edited = editDetectionDraft(reordered, 'b', {
      displayName: 'Edited B',
      command: 'edited command b',
      workingDirectory: 'edited-directory-b',
    });
    const before = JSON.stringify(edited);
    const draftA = edited.drafts[1];

    const reset = resetDetectionDraft(edited, 'b');

    expect(reset.drafts.map(({ stableId }) => stableId)).toEqual(['b', 'a']);
    expect(reset.drafts[0]).toMatchObject({
      stableId: 'b',
      displayName: 'Original B',
      command: 'command b',
      workingDirectory: 'directory-b',
    });
    expect(reset.drafts[1]).toBe(draftA);
    expect(reset.selectedIds).toBe(edited.selectedIds);
    expect([...reset.selectedIds]).toEqual(['b']);
    expect(reset.originalResult).toBe(edited.originalResult);
    expect(reset.originalResult.suggestions.map(({ stableId }) => stableId)).toEqual(['a', 'b']);
    expect(JSON.stringify(edited)).toBe(before);
  });

  it('is a safe no-op when a draft has no matching original suggestion', () => {
    const original = createDetectionReview(makeResult([makeSuggestion('orphan')]));
    const state = {
      ...original,
      originalResult: { ...original.originalResult, suggestions: [] },
    };

    expect(resetDetectionDraft(state, 'orphan')).toBe(state);
  });
});

describe('draft validation', () => {
  it('uses the shared service field rules', () => {
    const draft = createDetectionReview(makeResult([makeSuggestion('one')])).drafts[0]!;
    const invalid = {
      ...draft,
      displayName: ' ',
      command: 'echo\nmarker',
      workingDirectory: '../outside',
    };

    expect(validateDetectionDraft(invalid)).toEqual({
      displayName: validateServiceName(invalid.displayName),
      command: validateServiceCommand(invalid.command),
      workingDirectory: validateServiceWorkingDirectory(invalid.workingDirectory),
    });
  });

  it('reports whether a draft is valid', () => {
    const draft = createDetectionReview(makeResult([makeSuggestion('one')])).drafts[0]!;

    expect(isDetectionDraftValid(draft)).toBe(true);
    expect(isDetectionDraftValid({ ...draft, command: '' })).toBe(false);
  });
});

describe('warning classification', () => {
  const expectedSeverity = {
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
  } satisfies Record<DetectionWarningKind, DetectionWarningSeverity>;

  it('classifies all 15 warning kinds exactly', () => {
    const actual = Object.fromEntries(
      Object.keys(expectedSeverity).map((kind) => [
        kind,
        getDetectionWarningSeverity(kind as DetectionWarningKind),
      ]),
    );

    expect(Object.keys(expectedSeverity)).toHaveLength(15);
    expect(actual).toEqual(expectedSeverity);
  });

  it('detects registered services exclusively by warning kind', () => {
    const first = makeWarning('matchesRegisteredService', 'Already registered.');
    const second = makeWarning('matchesRegisteredService', 'Completely different copy.');
    const misleading = makeWarning('emptyNpmScript', 'matchesRegisteredService');

    expect(hasRegisteredServiceWarning([first])).toBe(true);
    expect(hasRegisteredServiceWarning([second])).toBe(true);
    expect(hasRegisteredServiceWarning([misleading])).toBe(false);
    expect(getDetectionWarningSeverity(first.kind)).toBe(getDetectionWarningSeverity(second.kind));
  });
});

describe('immutability', () => {
  it('does not mutate the input result or prior review states', () => {
    const result = makeResult([
      makeSuggestion('one', 'npmScript', { defaultSelected: true }),
      makeSuggestion('two', 'cmd'),
    ]);
    const before = JSON.stringify(result);
    const state = createDetectionReview(result);
    const selectedBefore = [...state.selectedIds];

    const edited = editDetectionDraft(state, 'one', { command: 'changed' });
    const selected = selectAllEligible(edited);
    resetDetectionDraft(selected, 'one');

    expect(JSON.stringify(result)).toBe(before);
    expect(state.originalResult).toBe(result);
    expect(state.drafts[0]?.command).toBe('run one');
    expect([...state.selectedIds]).toEqual(selectedBefore);
  });
});
