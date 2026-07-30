import { describe, expect, it, vi } from 'vitest';
import {
  applyRuntimeToRegistry,
  canStopRuntime,
  createRuntimeRegistry,
  isRuntimeForService,
  reconcileLogAppend,
  reconcileLogsClear,
  reconcileLogsSnapshot,
  reconcileRuntime,
} from './runtimeReconciliation';
import type {
  ServiceLogEntry,
  ServiceLogEvent,
  ServiceLogsSnapshot,
  ServiceRuntime,
} from './types';

const baseRuntime = (
  runId: string,
  runtimeRevision: number,
  status: ServiceRuntime['status'],
): ServiceRuntime => ({
  projectId: 'project',
  serviceId: 'service',
  runId,
  runtimeRevision,
  status,
  pid: status === 'running' ? 123 : null,
  startedAt: status === 'running' ? '2026-01-01T00:00:00.000Z' : null,
  exitCode: status === 'exited' ? 0 : null,
  error: null,
});

const runtimeForService = (
  serviceId: string,
  runId: string,
  runtimeRevision: number,
  status: ServiceRuntime['status'],
): ServiceRuntime => ({
  ...baseRuntime(runId, runtimeRevision, status),
  serviceId,
});

const entry = (sequence: number, text = `line-${sequence}`): ServiceLogEntry => ({
  sequence,
  timestamp: '2026-01-01T00:00:00.000Z',
  source: 'stdout',
  text,
});

const snapshot = (
  runId: string | null,
  logsRevision: number,
  entries: ServiceLogEntry[],
): ServiceLogsSnapshot => ({
  projectId: 'project',
  serviceId: 'service',
  runId,
  logsRevision,
  entries,
});

const append = (runId: string, logsRevision: number, sequence: number): ServiceLogEvent => ({
  projectId: 'project',
  serviceId: 'service',
  runId,
  logsRevision,
  entry: entry(sequence),
});

function applyRuntime(
  current: ServiceRuntime | undefined,
  incoming: ServiceRuntime,
  latestRevision = current?.runtimeRevision ?? 0,
): ServiceRuntime | undefined {
  return reconcileRuntime(current, incoming, latestRevision);
}

describe('runtime registry application', () => {
  it('accepts Running revision 3 into an empty registry with its PID', () => {
    const running = baseRuntime('run-3', 3, 'running');
    const result = applyRuntimeToRegistry(createRuntimeRegistry(), running);

    expect(result.accepted).toBe(true);
    expect(result.registry.runtimes.service).toBe(running);
    expect(result.registry.runtimes.service.status).toBe('running');
    expect(result.registry.runtimes.service.pid).toBe(123);
    expect(result.registry.latestRuntimeRevision.service).toBe(3);
  });

  it('discards a duplicate revision without removing the accepted Running snapshot', () => {
    const running = baseRuntime('run-3', 3, 'running');
    const accepted = applyRuntimeToRegistry(createRuntimeRegistry(), running);
    const duplicate = applyRuntimeToRegistry(accepted.registry, running);

    expect(duplicate.accepted).toBe(false);
    expect(duplicate.registry).toBe(accepted.registry);
    expect(duplicate.registry.runtimes.service).toBe(running);
  });

  it('is pure and deterministic when the same calculation is invoked twice like StrictMode', () => {
    const initial = createRuntimeRegistry();
    Object.freeze(initial.runtimes);
    Object.freeze(initial.latestRuntimeRevision);
    Object.freeze(initial);
    const running = baseRuntime('run-3', 3, 'running');

    const firstInvocation = applyRuntimeToRegistry(initial, running);
    const secondInvocation = applyRuntimeToRegistry(initial, running);

    expect(initial.runtimes).toEqual({});
    expect(initial.latestRuntimeRevision).toEqual({});
    expect(firstInvocation.accepted).toBe(true);
    expect(secondInvocation.accepted).toBe(true);
    expect(secondInvocation.registry).toEqual(firstInvocation.registry);

    const replayAfterCommit = applyRuntimeToRegistry(firstInvocation.registry, running);
    expect(replayAfterCommit.accepted).toBe(false);
    expect(replayAfterCommit.registry).toBe(firstInvocation.registry);
  });

  it('discards Stopped revision 2 after Running revision 3', () => {
    const running = applyRuntimeToRegistry(
      createRuntimeRegistry(),
      baseRuntime('run-3', 3, 'running'),
    );
    const staleStopped = applyRuntimeToRegistry(
      running.registry,
      baseRuntime('run-3', 2, 'stopped'),
    );

    expect(staleStopped.accepted).toBe(false);
    expect(staleStopped.registry.runtimes.service.status).toBe('running');
  });

  it('accepts a terminal revision 4 after Running revision 3', () => {
    const running = applyRuntimeToRegistry(
      createRuntimeRegistry(),
      baseRuntime('run-3', 3, 'running'),
    );
    const terminal = baseRuntime('run-3', 4, 'exited');
    const result = applyRuntimeToRegistry(running.registry, terminal);

    expect(result.accepted).toBe(true);
    expect(result.registry.runtimes.service).toBe(terminal);
    expect(result.registry.latestRuntimeRevision.service).toBe(4);
  });

  it('keeps a newer event when an older GET arrives afterward', () => {
    const event = baseRuntime('run-4', 4, 'running');
    const afterEvent = applyRuntimeToRegistry(createRuntimeRegistry(), event);
    const oldGet = applyRuntimeToRegistry(afterEvent.registry, baseRuntime('run-3', 3, 'stopped'));

    expect(oldGet.accepted).toBe(false);
    expect(oldGet.registry.runtimes.service).toBe(event);
  });

  it('ends with a newer event when it follows an older GET', () => {
    const get = baseRuntime('run-3', 3, 'stopped');
    const afterGet = applyRuntimeToRegistry(createRuntimeRegistry(), get);
    const event = baseRuntime('run-4', 4, 'running');
    const afterEvent = applyRuntimeToRegistry(afterGet.registry, event);

    expect(afterEvent.accepted).toBe(true);
    expect(afterEvent.registry.runtimes.service).toBe(event);
  });

  it('tracks two service revisions independently without replacing the other snapshot', () => {
    const first = runtimeForService('first', 'A', 3, 'running');
    const second = runtimeForService('second', 'B', 7, 'running');
    const afterFirst = applyRuntimeToRegistry(createRuntimeRegistry(), first);
    const afterSecond = applyRuntimeToRegistry(afterFirst.registry, second);
    const firstTerminal = runtimeForService('first', 'A', 4, 'exited');
    const result = applyRuntimeToRegistry(afterSecond.registry, firstTerminal);

    expect(result.registry.runtimes.first).toBe(firstTerminal);
    expect(result.registry.runtimes.second).toBe(second);
    expect(result.registry.latestRuntimeRevision).toEqual({ first: 4, second: 7 });
  });

  it('does not replace a snapshot with different data at the same revision', () => {
    const running = baseRuntime('run-3', 3, 'running');
    const accepted = applyRuntimeToRegistry(createRuntimeRegistry(), running);
    const conflicting = {
      ...baseRuntime('run-3', 3, 'failed'),
      error: 'same revision, different payload',
    };
    const result = applyRuntimeToRegistry(accepted.registry, conflicting);

    expect(result.accepted).toBe(false);
    expect(result.registry.runtimes.service).toBe(running);
  });

  it('uses the same application path for GET, start, event, and stop responses', () => {
    const snapshots: Array<[string, ServiceRuntime]> = [
      ['GET', baseRuntime('run', 1, 'stopped')],
      ['start', baseRuntime('run', 2, 'starting')],
      ['event', baseRuntime('run', 3, 'running')],
      ['stop', baseRuntime('run', 4, 'stopping')],
    ];
    let registry = createRuntimeRegistry();

    for (const [source, runtime] of snapshots) {
      const result = applyRuntimeToRegistry(registry, runtime);
      expect(result.accepted, source).toBe(true);
      registry = result.registry;
    }

    expect(registry.runtimes.service.status).toBe('stopping');
    expect(registry.latestRuntimeRevision.service).toBe(4);
  });

  it('accepts a remount GET once and a duplicate cannot erase it', () => {
    const remountedRegistry = createRuntimeRegistry();
    const running = baseRuntime('run-3', 3, 'running');
    const hydrated = applyRuntimeToRegistry(remountedRegistry, running);
    const strictModeReplay = applyRuntimeToRegistry(hydrated.registry, running);

    expect(hydrated.accepted).toBe(true);
    expect(strictModeReplay.accepted).toBe(false);
    expect(strictModeReplay.registry.runtimes.service).toBe(running);
  });

  it('enables Stop from the accepted Running snapshot with a PID', () => {
    const result = applyRuntimeToRegistry(
      createRuntimeRegistry(),
      baseRuntime('run-3', 3, 'running'),
    );

    expect(canStopRuntime(result.registry.runtimes.service)).toBe(true);
  });
});

describe('runtime reconciliation', () => {
  it('accepts Running(B) with a higher revision while Running(A) is displayed', () => {
    const current = baseRuntime('A', 3, 'running');
    const incoming = baseRuntime('B', 5, 'running');
    expect(applyRuntime(current, incoming, 3)).toBe(incoming);
  });

  it('discards delayed Exited(A) after accepting Running(B)', () => {
    const runningB = baseRuntime('B', 5, 'running');
    const exitedA = baseRuntime('A', 4, 'exited');
    const current = applyRuntime(baseRuntime('A', 3, 'running'), runningB, 3);
    expect(applyRuntime(current, exitedA, current?.runtimeRevision)).toBe(current);
  });

  it('treats duplicate Running(B) revisions as idempotent', () => {
    const current = baseRuntime('B', 5, 'running');
    expect(applyRuntime(current, baseRuntime('B', 5, 'running'), 5)).toBe(current);
  });

  it('discards an older snapshot from Run A', () => {
    const current = baseRuntime('B', 5, 'running');
    expect(applyRuntime(current, baseRuntime('A', 2, 'exited'), 5)).toBe(current);
  });

  it('accepts a higher-revision IPC response for B while A appears active', () => {
    const current = baseRuntime('A', 3, 'running');
    const ipcResponse = baseRuntime('B', 5, 'running');
    expect(applyRuntime(current, ipcResponse, 3)).toBe(ipcResponse);
  });

  it('accepts a higher-revision runtime event for B while A appears active', () => {
    const current = baseRuntime('A', 3, 'running');
    const event = baseRuntime('B', 5, 'running');
    expect(applyRuntime(current, event, 3)).toBe(event);
  });

  it('guards another service response before reconciliation', () => {
    const reconcile = vi.fn(reconcileRuntime);
    const current = runtimeForService('service', 'A', 3, 'running');
    const incoming = runtimeForService('other-service', 'B', 5, 'running');
    const next = isRuntimeForService(incoming, 'project', 'service')
      ? reconcile(current, incoming, current.runtimeRevision)
      : current;
    expect(next).toBe(current);
    expect(reconcile).not.toHaveBeenCalled();
  });

  it('accepts a legitimate terminal transition for the current run', () => {
    const current = baseRuntime('B', 5, 'running');
    const exited = baseRuntime('B', 6, 'exited');
    expect(applyRuntime(current, exited, 5)).toBe(exited);
  });

  it('converges to Running(B) when Running(B) arrives before delayed Exited(A)', () => {
    let current: ServiceRuntime | undefined = baseRuntime('A', 3, 'running');
    const delayedExitedA = baseRuntime('A', 4, 'exited');
    const runningB = baseRuntime('B', 5, 'running');
    current = applyRuntime(current, runningB, current.runtimeRevision) ?? current;
    expect(current).toBe(runningB);
    current = applyRuntime(current, delayedExitedA, current.runtimeRevision) ?? current;
    expect(current).toBe(runningB);
  });

  it('converges to Running(B) when Exited(A) arrives before Running(B)', () => {
    let current: ServiceRuntime | undefined = baseRuntime('A', 3, 'running');
    const exitedA = baseRuntime('A', 4, 'exited');
    const runningB = baseRuntime('B', 5, 'running');
    current = applyRuntime(current, exitedA, current.runtimeRevision) ?? current;
    expect(current).toBe(exitedA);
    current = applyRuntime(current, runningB, current.runtimeRevision) ?? current;
    expect(current).toBe(runningB);
  });

  it('keeps same-revision callbacks idempotent', () => {
    const current = baseRuntime('B', 5, 'running');
    const first = applyRuntime(current, baseRuntime('B', 5, 'running'), 5);
    const second = applyRuntime(first, baseRuntime('B', 5, 'running'), 5);
    expect(first).toBe(current);
    expect(second).toBe(current);
  });

  it('discards lower runtime revisions', () => {
    const current = baseRuntime('B', 5, 'running');
    expect(applyRuntime(current, baseRuntime('B', 4, 'stopping'), 5)).toBe(current);
  });

  it('discards an old get_service_runtime response after a newer event', () => {
    const current = baseRuntime('B', 8, 'running');
    expect(applyRuntime(current, baseRuntime('A', 7, 'exited'), 8)).toBe(current);
  });

  it('accepts a later valid revision after discarding an old one', () => {
    const current = baseRuntime('B', 8, 'running');
    const afterOld = applyRuntime(current, baseRuntime('A', 7, 'exited'), 8);
    const stopping = baseRuntime('B', 9, 'stopping');
    expect(applyRuntime(afterOld, stopping, 8)).toBe(stopping);
  });
});

describe('logs reconciliation', () => {
  it('keeps logs empty for Clear 20 followed by append 19', () => {
    const cleared = reconcileLogsClear(snapshot('B', 18, [entry(1)]), snapshot('B', 20, []), 18);
    expect(reconcileLogAppend(cleared, append('B', 19, 2), cleared?.logsRevision)).toEqual(cleared);
  });

  it('leaves logs empty for append 19 followed by Clear 20', () => {
    const appended = reconcileLogAppend(snapshot('B', 18, []), append('B', 19, 1), 18);
    const cleared = reconcileLogsClear(appended, snapshot('B', 20, []), appended?.logsRevision);
    expect(cleared?.entries).toEqual([]);
    expect(cleared?.logsRevision).toBe(20);
  });

  it('discards a logs snapshot older than Clear 20', () => {
    const cleared = snapshot('B', 20, []);
    expect(reconcileLogsSnapshot(cleared, snapshot('B', 18, [entry(1)]), 20)).toBe(cleared);
  });

  it('does not append Run A to Run B', () => {
    const current = snapshot('B', 20, []);
    expect(reconcileLogAppend(current, append('A', 21, 1), 20)).toBe(current);
  });

  it('does not apply event and response Clear with same revision twice', () => {
    const cleared = reconcileLogsClear(snapshot('B', 19, [entry(1)]), snapshot('B', 20, []), 19);
    expect(reconcileLogsClear(cleared, snapshot('B', 20, []), 20)).toBe(cleared);
  });

  it('converges for initial snapshot and events in different arrival orders', () => {
    const initial = snapshot('B', 10, []);
    const event = append('B', 11, 2);
    const snapshotThenEvent = reconcileLogAppend(
      reconcileLogsSnapshot(null, initial, 0),
      event,
      10,
    );
    const eventThenSnapshot = reconcileLogsSnapshot(
      reconcileLogAppend(null, event, 0),
      initial,
      11,
    );
    expect(snapshotThenEvent).toEqual(eventThenSnapshot);
  });

  it('uses the latest revision supplied by asynchronous listeners', () => {
    let latest = 20;
    const current = snapshot('B', 20, []);
    const staleClosureLatest = 18;
    expect(reconcileLogAppend(current, append('B', 19, 1), latest)).toBe(current);
    expect(
      reconcileLogAppend(current, append('B', 19, 1), staleClosureLatest)?.entries,
    ).toHaveLength(1);
    latest = 21;
    expect(reconcileLogAppend(current, append('B', 20, 1), latest)).toBe(current);
  });
});
