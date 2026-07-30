import { describe, expect, it } from 'vitest';
import {
  applyLogAppendToRegistry,
  applyLogsSnapshotToRegistry,
  areLogsLoaded,
  beginClearInRegistry,
  createLogsRegistry,
  logsEntryForService,
  logsSnapshotForService,
  rejectClearInRegistry,
  resolveClearInRegistry,
  retainLogsRegistryServices,
  type LogsRegistry,
} from './logsReconciliation';
import type { ServiceLogEntry, ServiceLogEvent, ServiceLogsSnapshot } from './types';

type SnapshotOptions = Partial<Omit<ServiceLogsSnapshot, 'entries' | 'logsRevision'>> & {
  logsRevision: number;
  entries?: ServiceLogEntry[];
};
type AppendOptions = Partial<Omit<ServiceLogEvent, 'entry' | 'logsRevision'>> & {
  logsRevision: number;
  sequence: number;
  text?: string;
};
const entry = (sequence: number, text = `line-${sequence}`): ServiceLogEntry => ({
  sequence,
  timestamp: '2026-01-01T00:00:00.000Z',
  source: sequence % 2 ? 'stdout' : 'stderr',
  text,
});
const snapshot = ({
  projectId = 'project',
  serviceId = 'service',
  runId = 'run-a',
  logsRevision,
  entries = [],
}: SnapshotOptions): ServiceLogsSnapshot => ({
  projectId,
  serviceId,
  runId,
  logsRevision,
  entries,
});
const append = ({
  projectId = 'project',
  serviceId = 'service',
  runId = 'run-a',
  logsRevision,
  sequence,
  text,
}: AppendOptions): ServiceLogEvent => ({
  projectId,
  serviceId,
  runId,
  logsRevision,
  entry: entry(sequence, text),
});
const seed = (options: SnapshotOptions): LogsRegistry =>
  applyLogsSnapshotToRegistry(createLogsRegistry(), snapshot(options)).registry;
const visible = (registry: LogsRegistry, projectId = 'project', serviceId = 'service') =>
  logsSnapshotForService(registry, projectId, serviceId);
const beginAtFour = () =>
  beginClearInRegistry(seed({ logsRevision: 4, entries: [entry(1, 'old')] }), 'project', 'service');
const bufferSix = (registry: LogsRegistry, runId = 'run-a') =>
  applyLogAppendToRegistry(registry, append({ runId, logsRevision: 6, sequence: 2, text: 'new' }));
const clearFive = (registry: LogsRegistry, runId = 'run-a', requestId?: number) =>
  resolveClearInRegistry(registry, snapshot({ runId, logsRevision: 5 }), requestId);

describe('LogsRegistry StrictMode and revision zero', () => {
  it('1 accepts snapshot revision 1 on an empty registry', () => {
    const result = applyLogsSnapshotToRegistry(
      createLogsRegistry(),
      snapshot({ logsRevision: 1, entries: [entry(1)] }),
    );
    expect(result.accepted).toBe(true);
    expect(result.visibleSnapshot?.entries).toHaveLength(1);
    expect(logsEntryForService(result.registry, 'project', 'service')).toMatchObject({
      latestLogsRevision: 1,
      loaded: true,
    });
  });

  it('2 repeated snapshot does not revert the registry', () => {
    const payload = snapshot({ logsRevision: 1, entries: [entry(1)] });
    const first = applyLogsSnapshotToRegistry(createLogsRegistry(), payload);
    const duplicate = applyLogsSnapshotToRegistry(first.registry, payload);
    expect(duplicate.accepted).toBe(false);
    expect(duplicate.registry).toBe(first.registry);
    expect(duplicate.visibleSnapshot).toBe(first.visibleSnapshot);
  });

  it('3 StrictMode-style double invocation is pure and keeps entries', () => {
    const initial = Object.freeze(createLogsRegistry());
    const payload = Object.freeze(snapshot({ logsRevision: 1, entries: [entry(1)] }));
    const first = applyLogsSnapshotToRegistry(initial, payload);
    const replay = applyLogsSnapshotToRegistry(initial, payload);
    expect(initial.entries).toEqual({});
    expect(replay.registry).toEqual(first.registry);
    expect(applyLogsSnapshotToRegistry(first.registry, payload).registry).toBe(first.registry);
    expect(visible(first.registry)?.entries).toHaveLength(1);
  });

  it('4 accepts append revision 2 once', () => {
    const result = applyLogAppendToRegistry(
      seed({ logsRevision: 1, entries: [entry(1)] }),
      append({ logsRevision: 2, sequence: 2 }),
    );
    expect(result.accepted).toBe(true);
    expect(result.visibleSnapshot?.entries.map((item) => item.sequence)).toEqual([1, 2]);
  });

  it('5 duplicate append revision 2 does not duplicate the entry', () => {
    const payload = append({ logsRevision: 2, sequence: 2 });
    const first = applyLogAppendToRegistry(seed({ logsRevision: 1, entries: [entry(1)] }), payload);
    const duplicate = applyLogAppendToRegistry(first.registry, payload);
    expect(duplicate.registry).toBe(first.registry);
    expect(duplicate.visibleSnapshot?.entries.map((item) => item.sequence)).toEqual([1, 2]);
  });

  it('6 accepts Clear revision 3 once', () => {
    const current = applyLogAppendToRegistry(
      seed({ logsRevision: 1, entries: [entry(1)] }),
      append({ logsRevision: 2, sequence: 2 }),
    );
    const result = resolveClearInRegistry(current.registry, snapshot({ logsRevision: 3 }));
    expect(result.accepted).toBe(true);
    expect(result.visibleSnapshot).toMatchObject({ logsRevision: 3, entries: [] });
  });

  it('7 duplicate Clear does not restore entries', () => {
    const payload = snapshot({ logsRevision: 3 });
    const first = resolveClearInRegistry(seed({ logsRevision: 2, entries: [entry(1)] }), payload);
    const duplicate = resolveClearInRegistry(first.registry, payload);
    expect(duplicate.accepted).toBe(false);
    expect(duplicate.registry).toBe(first.registry);
    expect(duplicate.visibleSnapshot?.entries).toEqual([]);
  });

  it('8 accepts snapshot revision 0 with empty entries as loaded', () => {
    const result = applyLogsSnapshotToRegistry(
      createLogsRegistry(),
      snapshot({ runId: null, logsRevision: 0 }),
    );
    expect(result.accepted).toBe(true);
    expect(logsEntryForService(result.registry, 'project', 'service')).toMatchObject({
      latestLogsRevision: 0,
      loaded: true,
      snapshot: { runId: null, entries: [] },
    });
  });

  it('9 repeated revision 0 is a duplicate', () => {
    const payload = snapshot({ runId: null, logsRevision: 0 });
    const first = applyLogsSnapshotToRegistry(createLogsRegistry(), payload);
    const duplicate = applyLogsSnapshotToRegistry(first.registry, payload);
    expect(duplicate.accepted).toBe(false);
    expect(duplicate.registry).toBe(first.registry);
  });

  it('10 selected UI data is loaded empty rather than Loading', () => {
    const registry = seed({ runId: null, logsRevision: 0 });
    expect(areLogsLoaded(registry, 'project', 'service')).toBe(true);
    expect(visible(registry)).not.toBeNull();
    expect(visible(registry)?.entries).toEqual([]);
  });
});

describe('LogsRegistry Clear ordering', () => {
  it('11 represents revision 4 with old entries', () => {
    expect(visible(seed({ logsRevision: 4, entries: [entry(1, 'old')] }))).toMatchObject({
      logsRevision: 4,
      entries: [{ text: 'old' }],
    });
  });

  it('12 beginClear preserves visible state and records the barrier', () => {
    const original = seed({ logsRevision: 4, entries: [entry(1, 'old')] });
    const begun = beginClearInRegistry(original, 'project', 'service');
    expect(begun.accepted).toBe(true);
    expect(begun.visibleSnapshot).toBe(visible(original));
    expect(begun.pendingClear).toMatchObject({ baseRevision: 4, baseRunId: 'run-a' });
    expect(logsEntryForService(begun.registry, 'project', 'service')?.latestLogsRevision).toBe(4);
  });

  it('13 append revision 6 is buffered before Clear revision 5 arrives', () => {
    const buffered = bufferSix(beginAtFour().registry);
    expect(buffered.accepted).toBe(true);
    expect(buffered.visibleSnapshot?.entries[0].text).toBe('old');
    expect(buffered.pendingClear?.bufferedAppends.map((item) => item.logsRevision)).toEqual([6]);
    expect(logsEntryForService(buffered.registry, 'project', 'service')?.latestLogsRevision).toBe(
      4,
    );
  });

  it('14 resolving Clear revision 5 replays append revision 6', () => {
    const begun = beginAtFour();
    const resolved = clearFive(
      bufferSix(begun.registry).registry,
      'run-a',
      begun.clearRequestId ?? -1,
    );
    expect(resolved.accepted).toBe(true);
    expect(resolved.pendingClear).toBeNull();
    expect(resolved.visibleSnapshot).toMatchObject({
      logsRevision: 6,
      entries: [{ text: 'new' }],
    });
  });

  it('15 confirmed Clear removes every pre-Clear entry and keeps post-Clear entries', () => {
    const resolved = clearFive(bufferSix(beginAtFour().registry).registry);
    expect(resolved.visibleSnapshot?.entries.map((item) => item.text)).toEqual(['new']);
    expect(resolved.visibleSnapshot?.entries.some((item) => item.text === 'old')).toBe(false);
  });

  it('16 delayed revision 4 is discarded while Clear is pending', () => {
    const begun = beginAtFour();
    const stale = applyLogAppendToRegistry(
      begun.registry,
      append({ logsRevision: 4, sequence: 2, text: 'stale' }),
    );
    expect(stale.accepted).toBe(false);
    expect(clearFive(stale.registry).visibleSnapshot?.entries).toEqual([]);
  });

  it('17 revisions 6, 7, and 8 replay once in order', () => {
    let registry = beginAtFour().registry;
    for (const revision of [8, 6, 7, 7]) {
      registry = applyLogAppendToRegistry(
        registry,
        append({ logsRevision: revision, sequence: revision, text: `r${revision}` }),
      ).registry;
    }
    expect(clearFive(registry).visibleSnapshot?.entries.map((item) => item.text)).toEqual([
      'r6',
      'r7',
      'r8',
    ]);
  });

  it('18 Clear event before response is idempotent', () => {
    const begun = beginAtFour();
    const payload = snapshot({ logsRevision: 5 });
    const eventResult = resolveClearInRegistry(bufferSix(begun.registry).registry, payload);
    const responseResult = resolveClearInRegistry(
      eventResult.registry,
      payload,
      begun.clearRequestId ?? -1,
    );
    expect(responseResult.accepted).toBe(false);
    expect(responseResult.registry).toBe(eventResult.registry);
    expect(responseResult.visibleSnapshot?.entries[0].text).toBe('new');
  });

  it('19 Clear response before event is idempotent', () => {
    const begun = beginAtFour();
    const payload = snapshot({ logsRevision: 5 });
    const response = resolveClearInRegistry(begun.registry, payload, begun.clearRequestId ?? -1);
    const appended = applyLogAppendToRegistry(
      response.registry,
      append({ logsRevision: 6, sequence: 2, text: 'new' }),
    );
    const eventResult = resolveClearInRegistry(appended.registry, payload);
    expect(eventResult.accepted).toBe(false);
    expect(eventResult.registry).toBe(appended.registry);
    expect(eventResult.visibleSnapshot?.entries[0].text).toBe('new');
  });

  it('20 failed Clear restores the barrier and replays retained appends', () => {
    const begun = beginAtFour();
    const rejected = rejectClearInRegistry(
      bufferSix(begun.registry).registry,
      'project',
      'service',
      begun.clearRequestId ?? -1,
    );
    expect(rejected.accepted).toBe(true);
    expect(rejected.pendingClear).toBeNull();
    expect(rejected.visibleSnapshot).toMatchObject({
      logsRevision: 6,
      entries: [{ text: 'old' }, { text: 'new' }],
    });
  });

  it('21 consecutive Clear barriers are independent and the second dominates', () => {
    const first = beginAtFour();
    const firstResolved = clearFive(
      bufferSix(first.registry).registry,
      'run-a',
      first.clearRequestId ?? -1,
    );
    const second = beginClearInRegistry(firstResolved.registry, 'project', 'service');
    const afterSecond = applyLogAppendToRegistry(
      second.registry,
      append({ logsRevision: 8, sequence: 3, text: 'after-second' }),
    );
    const result = resolveClearInRegistry(
      afterSecond.registry,
      snapshot({ logsRevision: 7 }),
      second.clearRequestId ?? -1,
    );
    expect(second.clearRequestId).not.toBe(first.clearRequestId);
    expect(result.visibleSnapshot?.entries.map((item) => item.text)).toEqual(['after-second']);
  });

  it('22 changing service during Clear never mixes snapshots', () => {
    let registry = seed({ serviceId: 'first', logsRevision: 4, entries: [entry(1, 'first-old')] });
    registry = applyLogsSnapshotToRegistry(
      registry,
      snapshot({ serviceId: 'second', logsRevision: 2, entries: [entry(10, 'second-old')] }),
    ).registry;
    const begun = beginClearInRegistry(registry, 'project', 'first');
    registry = applyLogAppendToRegistry(
      begun.registry,
      append({ serviceId: 'first', logsRevision: 6, sequence: 2, text: 'first-new' }),
    ).registry;
    registry = applyLogAppendToRegistry(
      registry,
      append({ serviceId: 'second', logsRevision: 3, sequence: 11, text: 'second-new' }),
    ).registry;
    registry = resolveClearInRegistry(
      registry,
      snapshot({ serviceId: 'first', logsRevision: 5 }),
    ).registry;
    expect(visible(registry, 'project', 'first')?.entries.map((item) => item.text)).toEqual([
      'first-new',
    ]);
    expect(visible(registry, 'project', 'second')?.entries.map((item) => item.text)).toEqual([
      'second-old',
      'second-new',
    ]);
  });

  it('23 same serviceId in two projects never collides', () => {
    let registry = seed({
      projectId: 'project-a',
      serviceId: 'shared',
      logsRevision: 1,
      entries: [entry(1, 'a')],
    });
    registry = applyLogsSnapshotToRegistry(
      registry,
      snapshot({
        projectId: 'project-b',
        serviceId: 'shared',
        logsRevision: 1,
        entries: [entry(1, 'b')],
      }),
    ).registry;
    expect(visible(registry, 'project-a', 'shared')?.entries[0].text).toBe('a');
    expect(visible(registry, 'project-b', 'shared')?.entries[0].text).toBe('b');
  });

  it('24 new runId during Clear cannot mix with the previous run', () => {
    const begun = beginClearInRegistry(
      seed({ runId: 'run-a', logsRevision: 4, entries: [entry(1, 'run-a')] }),
      'project',
      'service',
    );
    const buffered = bufferSix(begun.registry, 'run-b');
    const resolved = clearFive(buffered.registry, 'run-b');
    expect(resolved.visibleSnapshot?.runId).toBe('run-b');
    expect(resolved.visibleSnapshot?.entries.map((item) => item.text)).toEqual(['new']);
  });
});

describe('LogsRegistry general ordering and selection', () => {
  it('25 old snapshot after a newer append cannot overwrite it', () => {
    const appended = applyLogAppendToRegistry(
      seed({ logsRevision: 1, entries: [entry(1)] }),
      append({ logsRevision: 3, sequence: 3 }),
    );
    const stale = applyLogsSnapshotToRegistry(
      appended.registry,
      snapshot({ logsRevision: 2, entries: [entry(1), entry(2)] }),
    );
    expect(stale.accepted).toBe(false);
    expect(stale.registry).toBe(appended.registry);
    expect(stale.visibleSnapshot?.logsRevision).toBe(3);
  });

  it('26 old append after a newer snapshot is discarded', () => {
    const current = seed({ logsRevision: 3, entries: [entry(1), entry(2), entry(3)] });
    const stale = applyLogAppendToRegistry(current, append({ logsRevision: 2, sequence: 2 }));
    expect(stale.accepted).toBe(false);
    expect(stale.registry).toBe(current);
  });

  it('27 GET event and Clear response use the same registry', () => {
    const fromGet = applyLogsSnapshotToRegistry(
      createLogsRegistry(),
      snapshot({ logsRevision: 1, entries: [entry(1)] }),
    );
    const fromEvent = applyLogAppendToRegistry(
      fromGet.registry,
      append({ logsRevision: 2, sequence: 2 }),
    );
    const begun = beginClearInRegistry(fromEvent.registry, 'project', 'service');
    const fromResponse = resolveClearInRegistry(
      begun.registry,
      snapshot({ logsRevision: 3 }),
      begun.clearRequestId ?? -1,
    );
    expect([fromGet.accepted, fromEvent.accepted, fromResponse.accepted]).toEqual([
      true,
      true,
      true,
    ]);
    expect(fromResponse.visibleSnapshot).toMatchObject({ logsRevision: 3, entries: [] });
  });

  it('28 changing selection away and back preserves the canonical snapshot', () => {
    let registry = seed({ serviceId: 'first', logsRevision: 1, entries: [entry(1, 'first')] });
    registry = applyLogsSnapshotToRegistry(
      registry,
      snapshot({ serviceId: 'second', logsRevision: 1, entries: [entry(1, 'second')] }),
    ).registry;
    const before = visible(registry, 'project', 'first');
    expect(visible(registry, 'project', 'second')?.entries[0].text).toBe('second');
    expect(visible(registry, 'project', 'first')).toBe(before);
  });

  it('29 loaded empty remains loaded after duplicate and stale inputs', () => {
    const loaded = seed({ runId: null, logsRevision: 0 });
    const duplicate = applyLogsSnapshotToRegistry(
      loaded,
      snapshot({ runId: null, logsRevision: 0 }),
    );
    const stale = applyLogAppendToRegistry(
      duplicate.registry,
      append({ logsRevision: 0, sequence: 1 }),
    );
    expect(areLogsLoaded(stale.registry, 'project', 'service')).toBe(true);
    expect(stale.visibleSnapshot?.entries).toEqual([]);
  });

  it('30 rejected GET leaves loaded false and invents no entries', () => {
    const unchangedAfterRejectedGet = createLogsRegistry();
    expect(areLogsLoaded(unchangedAfterRejectedGet, 'project', 'service')).toBe(false);
    expect(visible(unchangedAfterRejectedGet)).toBeNull();
    expect(Object.keys(unchangedAfterRejectedGet.entries)).toHaveLength(0);
  });

  it('31 newer full snapshot during Clear remains authoritative after the boundary', () => {
    const begun = beginAtFour();
    const buffered = bufferSix(begun.registry);
    const fromNavigationGet = applyLogsSnapshotToRegistry(
      buffered.registry,
      snapshot({ logsRevision: 6, entries: [entry(2, 'new')] }),
    );
    const resolved = clearFive(fromNavigationGet.registry);
    expect(resolved.visibleSnapshot).toMatchObject({
      logsRevision: 6,
      entries: [{ text: 'new' }],
    });
  });

  it('32 retaining configured services does not change other snapshots', () => {
    let registry = seed({ serviceId: 'first', logsRevision: 1, entries: [entry(1)] });
    registry = applyLogsSnapshotToRegistry(
      registry,
      snapshot({ serviceId: 'second', logsRevision: 1, entries: [entry(2)] }),
    ).registry;
    const retained = retainLogsRegistryServices(registry, 'project', new Set(['second']));
    expect(visible(retained, 'project', 'first')).toBeNull();
    expect(visible(retained, 'project', 'second')?.entries[0].sequence).toBe(2);
  });
});
