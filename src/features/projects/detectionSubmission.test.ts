import { describe, expect, it } from 'vitest';
import {
  buildDetectionSubmissionPlan,
  draftToServiceInput,
  preparedDetectedServicesToSubmissions,
  type DetectionSubmissionPlan,
  type PreparedDetectedService,
} from './detectionSubmission';
import { validateDetectionDraft, type DetectionDraft } from './detectionDraft';
import type { ServiceDefinition } from './types';

function draft(stableId: string, overrides: Partial<DetectionDraft> = {}): DetectionDraft {
  return {
    stableId,
    sourceKind: 'npmScript',
    sourcePath: 'package.json',
    reason: `Detected ${stableId}`,
    warnings: [],
    defaultSelected: false,
    matchesRegisteredService: false,
    displayName: `Service ${stableId}`,
    command: `run ${stableId}`,
    workingDirectory: '.',
    ...overrides,
  };
}

function service(id: string, overrides: Partial<ServiceDefinition> = {}): ServiceDefinition {
  return {
    id,
    name: `Registered ${id}`,
    workingDirectory: '.',
    command: `run ${id}`,
    expectedPort: null,
    localUrl: null,
    createdAt: '2026-07-31T00:00:00Z',
    updatedAt: '2026-07-31T00:00:00Z',
    ...overrides,
  };
}

function plan(
  drafts: readonly DetectionDraft[],
  selectedIds: ReadonlySet<string>,
  registeredServices: readonly ServiceDefinition[] = [],
): DetectionSubmissionPlan {
  return buildDetectionSubmissionPlan({ drafts, selectedIds, registeredServices });
}

describe('draftToServiceInput', () => {
  it('trims editable fields, preserves internal spaces, and leaves optional values unknown', () => {
    const original = draft('one', {
      displayName: '  Sérvice  one  ',
      workingDirectory: '  apps/web  ',
      command: '  npm  run -- dev  ',
    });
    const before = structuredClone(original);

    const input = draftToServiceInput(original);

    expect(input).toEqual({
      name: 'Sérvice  one',
      workingDirectory: 'apps/web',
      command: 'npm  run -- dev',
      expectedPort: null,
      localUrl: null,
    });
    expect(input).not.toHaveProperty('stableId');
    expect(original).toEqual(before);
  });
});

describe('preparedDetectedServicesToSubmissions', () => {
  it('converts an empty collection without adding state', () => {
    expect(preparedDetectedServicesToSubmissions([])).toEqual([]);
  });

  it('preserves order, opaque IDs, input identity, nulls, and source immutability', () => {
    const firstInput = {
      name: 'First',
      workingDirectory: '.',
      command: 'run first',
      expectedPort: null,
      localUrl: null,
    };
    const secondInput = {
      name: 'Second',
      workingDirectory: 'apps/api',
      command: 'run second',
      expectedPort: 3_000,
      localUrl: 'http://localhost:3000',
    };
    const prepared = [
      { stableId: '  opaque-first  ', input: firstInput },
      { stableId: 'second', input: secondInput },
    ] satisfies readonly PreparedDetectedService[];
    const before = structuredClone(prepared);

    const submissions = preparedDetectedServicesToSubmissions(prepared);

    expect(submissions.map(({ stableId }) => stableId)).toEqual(['  opaque-first  ', 'second']);
    expect(submissions[0]?.service).toBe(firstInput);
    expect(submissions[1]?.service).toBe(secondInput);
    expect(submissions[0]?.service.expectedPort).toBeNull();
    expect(submissions[0]?.service.localUrl).toBeNull();
    expect(submissions[0]?.service).not.toHaveProperty('stableId');
    expect(Object.keys(submissions[0] ?? {})).toEqual(['stableId', 'service']);
    expect(prepared).toEqual(before);
  });
});

describe('selection and stale state', () => {
  it('returns an empty plan for an empty selection', () => {
    expect(plan([draft('one')], new Set())).toEqual({
      eligible: [],
      skipped: [],
      missingSelectedIds: [],
    });
  });

  it('ignores unselected drafts and does not let them reserve duplicate keys', () => {
    const drafts = [
      draft('ignored', { displayName: 'Shared', command: 'same' }),
      draft('selected', { displayName: 'shared', command: 'same' }),
    ];

    const result = plan(drafts, new Set(['selected']));

    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['selected']);
    expect(result.skipped).toEqual([]);
  });

  it('reports missing selected IDs in deterministic lexical order', () => {
    const result = plan([draft('present')], new Set(['zeta', 'present', 'alpha', 'Beta']));

    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['present']);
    expect(result.missingSelectedIds).toEqual(['Beta', 'alpha', 'zeta']);
  });
});

describe('registered service checks', () => {
  it('excludes an explicit registered match before validating the draft', () => {
    const matched = draft('matched', {
      matchesRegisteredService: true,
      displayName: '',
      command: '',
      workingDirectory: '',
    });

    expect(plan([matched], new Set(['matched'])).skipped).toEqual([
      {
        stableId: 'matched',
        displayName: '',
        kind: 'alreadyRegisteredMatch',
      },
    ]);
  });

  it('compares trimmed service names case-insensitively, including non-ASCII names', () => {
    const candidate = draft('candidate', { displayName: '  SÉRVIÇO  ' });
    const registered = service('existing', { name: 'sérviço' });

    expect(plan([candidate], new Set(['candidate']), [registered]).skipped).toEqual([
      {
        stableId: 'candidate',
        displayName: '  SÉRVIÇO  ',
        kind: 'duplicateExistingName',
      },
    ]);
  });

  it('compares working directory case-insensitively and command exactly after trim', () => {
    const registered = service('existing', {
      workingDirectory: 'Apps/Web',
      command: 'npm run -- dev',
    });
    const duplicate = draft('duplicate', {
      displayName: 'Duplicate functional service',
      workingDirectory: '  apps/web  ',
      command: '  npm run -- dev  ',
    });
    const differentCommandCase = draft('case-sensitive-command', {
      displayName: 'Different command case',
      workingDirectory: 'APPS/WEB',
      command: 'NPM run -- dev',
    });

    const result = plan(
      [duplicate, differentCommandCase],
      new Set(['duplicate', 'case-sensitive-command']),
      [registered],
    );

    expect(result.skipped.map(({ kind }) => kind)).toEqual([
      'duplicateExistingWorkingDirectoryCommand',
    ]);
    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['case-sensitive-command']);
  });

  it('prefers an existing name duplicate over an existing functional duplicate', () => {
    const registered = service('existing', {
      name: 'Same name',
      workingDirectory: 'client',
      command: 'run client',
    });
    const candidate = draft('candidate', {
      displayName: 'same NAME',
      workingDirectory: 'CLIENT',
      command: 'run client',
    });

    expect(plan([candidate], new Set(['candidate']), [registered]).skipped[0]).toMatchObject({
      kind: 'duplicateExistingName',
    });
  });
});

describe('draft validation', () => {
  it('excludes an edited invalid draft and preserves the shared structured errors', () => {
    const invalid = draft('invalid', {
      displayName: ' ',
      command: 'echo\nmarker',
      workingDirectory: '../outside',
    });
    const expectedErrors = validateDetectionDraft(invalid);
    const result = plan([invalid], new Set(['invalid']));
    const skipped = result.skipped[0];

    expect(skipped?.kind).toBe('invalidDraft');
    if (!skipped || skipped.kind !== 'invalidDraft') {
      throw new Error('Expected invalidDraft with structured errors.');
    }
    expect(skipped.errors).toEqual(expectedErrors);
  });

  it('checks invalidDraft before duplicates against registered services', () => {
    const invalid = draft('invalid', {
      displayName: 'Existing',
      command: '',
    });
    const registered = service('existing', { name: 'existing' });

    expect(plan([invalid], new Set(['invalid']), [registered]).skipped[0]).toMatchObject({
      kind: 'invalidDraft',
    });
  });

  it('does not reserve the name or functional key of an invalid earlier draft', () => {
    const invalid = draft('invalid', {
      displayName: 'Shared',
      command: '',
      workingDirectory: 'client',
    });
    const valid = draft('valid', {
      displayName: 'shared',
      command: 'run client',
      workingDirectory: 'CLIENT',
    });

    const result = plan([invalid, valid], new Set(['invalid', 'valid']));

    expect(result.skipped.map(({ stableId, kind }) => ({ stableId, kind }))).toEqual([
      { stableId: 'invalid', kind: 'invalidDraft' },
    ]);
    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['valid']);
  });
});

describe('duplicates inside one selection', () => {
  it('lets the first eligible name win and preserves eligible and skipped order', () => {
    const drafts = [
      draft('first', { displayName: 'Server', command: 'first command' }),
      draft('unrelated', { displayName: 'Worker', command: 'worker command' }),
      draft('duplicate', { displayName: 'server', command: 'other command' }),
      draft('last', { displayName: 'Proxy', command: 'proxy command' }),
    ];

    const result = plan(drafts, new Set(drafts.map(({ stableId }) => stableId)));

    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['first', 'unrelated', 'last']);
    expect(result.skipped.map(({ stableId, kind }) => ({ stableId, kind }))).toEqual([
      { stableId: 'duplicate', kind: 'duplicateSelectionName' },
    ]);
  });

  it('lets the first eligible workingDirectory and command pair win', () => {
    const first = draft('first', {
      displayName: 'First',
      workingDirectory: 'Apps/Api',
      command: 'node server.js',
    });
    const duplicate = draft('duplicate', {
      displayName: 'Second',
      workingDirectory: 'apps/api',
      command: 'node server.js',
    });

    const result = plan([first, duplicate], new Set(['first', 'duplicate']));

    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['first']);
    expect(result.skipped[0]).toMatchObject({
      stableId: 'duplicate',
      kind: 'duplicateSelectionWorkingDirectoryCommand',
    });
  });

  it('prefers an existing functional duplicate over a duplicate internal name', () => {
    const first = draft('first', {
      displayName: 'Shared',
      workingDirectory: 'first',
      command: 'first command',
    });
    const second = draft('second', {
      displayName: 'shared',
      workingDirectory: 'registered',
      command: 'registered command',
    });
    const registered = service('existing', {
      name: 'Different registered name',
      workingDirectory: 'REGISTERED',
      command: 'registered command',
    });

    const result = plan([first, second], new Set(['first', 'second']), [registered]);

    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['first']);
    expect(result.skipped[0]).toMatchObject({
      stableId: 'second',
      kind: 'duplicateExistingWorkingDirectoryCommand',
    });
  });

  it('does not reserve keys for a draft excluded by an explicit registered match', () => {
    const excluded = draft('excluded', {
      matchesRegisteredService: true,
      displayName: 'Shared',
      workingDirectory: 'client',
      command: 'run client',
    });
    const later = draft('later', {
      displayName: 'shared',
      workingDirectory: 'CLIENT',
      command: 'run client',
    });

    const result = plan([excluded, later], new Set(['excluded', 'later']));

    expect(result.skipped.map(({ stableId, kind }) => ({ stableId, kind }))).toEqual([
      { stableId: 'excluded', kind: 'alreadyRegisteredMatch' },
    ]);
    expect(result.eligible.map(({ stableId }) => stableId)).toEqual(['later']);
  });

  it('prefers an internal name duplicate over an internal functional duplicate', () => {
    const first = draft('first', {
      displayName: 'Shared',
      workingDirectory: 'client',
      command: 'run client',
    });
    const second = draft('second', {
      displayName: 'shared',
      workingDirectory: 'CLIENT',
      command: 'run client',
    });

    expect(plan([first, second], new Set(['first', 'second'])).skipped[0]).toMatchObject({
      kind: 'duplicateSelectionName',
    });
  });
});

describe('purity and correlation', () => {
  it('preserves stableId only as plan correlation metadata', () => {
    const result = plan([draft('stable-id')], new Set(['stable-id']));

    expect(result.eligible[0]?.stableId).toBe('stable-id');
    expect(result.eligible[0]?.input).not.toHaveProperty('stableId');
  });

  it('places every selected draft in at most one result collection', () => {
    const drafts = [
      draft('eligible'),
      draft('invalid', { command: '' }),
      draft('matched', { matchesRegisteredService: true }),
    ];
    const result = plan(drafts, new Set(['eligible', 'invalid', 'matched']));
    const eligibleIds = new Set(result.eligible.map(({ stableId }) => stableId));
    const skippedIds = new Set(result.skipped.map(({ stableId }) => stableId));

    expect([...eligibleIds].filter((stableId) => skippedIds.has(stableId))).toEqual([]);
    expect(result.eligible).toHaveLength(1);
    expect(result.skipped).toHaveLength(2);
    expect(result.skipped.map(({ stableId }) => stableId)).toEqual(['invalid', 'matched']);
  });

  it('does not mutate drafts, selection, or registered services', () => {
    const drafts = [draft('one'), draft('two', { displayName: 'Service one' })];
    const selectedIds = new Set(['one', 'two', 'missing']);
    const registeredServices = [service('existing')];
    const draftsBefore = structuredClone(drafts);
    const selectedBefore = [...selectedIds];
    const registeredBefore = structuredClone(registeredServices);

    buildDetectionSubmissionPlan({ drafts, selectedIds, registeredServices });

    expect(drafts).toEqual(draftsBefore);
    expect([...selectedIds]).toEqual(selectedBefore);
    expect(registeredServices).toEqual(registeredBefore);
  });
});
