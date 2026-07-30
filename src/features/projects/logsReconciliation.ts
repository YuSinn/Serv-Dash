import { limitEntries } from './runtimeReconciliation';
import type { ServiceLogEvent, ServiceLogsSnapshot } from './types';

export interface PendingLogsClear {
  readonly requestId: number;
  readonly baseRevision: number | null;
  readonly baseRunId: string | null;
  readonly bufferedAppends: readonly ServiceLogEvent[];
}

export interface LogsRegistryEntry {
  readonly projectId: string;
  readonly serviceId: string;
  readonly snapshot: ServiceLogsSnapshot | null;
  readonly latestLogsRevision: number | null;
  readonly loaded: boolean;
  readonly clearPending: PendingLogsClear | null;
}

export interface LogsRegistry {
  readonly entries: Readonly<Record<string, LogsRegistryEntry>>;
  readonly nextClearRequestId: number;
}

export interface LogsRegistryApplication {
  readonly registry: LogsRegistry;
  readonly accepted: boolean;
  readonly visibleSnapshot: ServiceLogsSnapshot | null;
  readonly pendingClear: PendingLogsClear | null;
}

export interface BeginLogsClearApplication extends LogsRegistryApplication {
  readonly clearRequestId: number | null;
}

export function logsRegistryKey(projectId: string, serviceId: string): string {
  return JSON.stringify([projectId, serviceId]);
}

export function createLogsRegistry(): LogsRegistry {
  return { entries: {}, nextClearRequestId: 1 };
}

export function logsEntryForService(
  registry: LogsRegistry,
  projectId: string,
  serviceId: string,
): LogsRegistryEntry | undefined {
  return registry.entries[logsRegistryKey(projectId, serviceId)];
}

export function logsSnapshotForService(
  registry: LogsRegistry,
  projectId: string,
  serviceId: string | null,
): ServiceLogsSnapshot | null {
  if (serviceId === null) return null;
  const entry = logsEntryForService(registry, projectId, serviceId);
  return entry?.loaded === true ? entry.snapshot : null;
}

export function areLogsLoaded(
  registry: LogsRegistry,
  projectId: string,
  serviceId: string,
): boolean {
  return logsEntryForService(registry, projectId, serviceId)?.loaded ?? false;
}

function createEntry(projectId: string, serviceId: string): LogsRegistryEntry {
  return {
    projectId,
    serviceId,
    snapshot: null,
    latestLogsRevision: null,
    loaded: false,
    clearPending: null,
  };
}

function applicationFor(
  registry: LogsRegistry,
  projectId: string,
  serviceId: string,
  accepted: boolean,
): LogsRegistryApplication {
  const entry = logsEntryForService(registry, projectId, serviceId);
  return {
    registry,
    accepted,
    visibleSnapshot: entry?.loaded === true ? entry.snapshot : null,
    pendingClear: entry?.clearPending ?? null,
  };
}

function withEntry(registry: LogsRegistry, entry: LogsRegistryEntry): LogsRegistry {
  return {
    ...registry,
    entries: { ...registry.entries, [logsRegistryKey(entry.projectId, entry.serviceId)]: entry },
  };
}

function isNewerRevision(incoming: number, latest: number | null): boolean {
  return latest === null || incoming > latest;
}

function normalizeSnapshot(
  snapshot: ServiceLogsSnapshot,
  forceEmpty: boolean,
): ServiceLogsSnapshot {
  return { ...snapshot, entries: forceEmpty ? [] : limitEntries(snapshot.entries) };
}

function appendToCommittedEntry(
  entry: LogsRegistryEntry,
  event: ServiceLogEvent,
): LogsRegistryEntry {
  const existingEntries =
    entry.loaded && entry.snapshot?.runId === event.runId ? entry.snapshot.entries : [];
  const snapshot: ServiceLogsSnapshot = {
    projectId: event.projectId,
    serviceId: event.serviceId,
    runId: event.runId,
    logsRevision: event.logsRevision,
    entries: limitEntries([...existingEntries, event.entry]),
  };
  return {
    ...entry,
    snapshot,
    latestLogsRevision: event.logsRevision,
    loaded: true,
    clearPending: null,
  };
}

function replayAppends(
  entry: LogsRegistryEntry,
  appends: readonly ServiceLogEvent[],
): LogsRegistryEntry {
  let next: LogsRegistryEntry = { ...entry, clearPending: null };
  const ordered = [...appends].sort((left, right) => left.logsRevision - right.logsRevision);
  for (const event of ordered) {
    if (event.projectId !== next.projectId || event.serviceId !== next.serviceId) continue;
    if (!isNewerRevision(event.logsRevision, next.latestLogsRevision)) continue;
    next = appendToCommittedEntry(next, event);
  }
  return next;
}

export function applyLogsSnapshotToRegistry(
  registry: LogsRegistry,
  incoming: ServiceLogsSnapshot,
): LogsRegistryApplication {
  const current =
    logsEntryForService(registry, incoming.projectId, incoming.serviceId) ??
    createEntry(incoming.projectId, incoming.serviceId);
  if (!isNewerRevision(incoming.logsRevision, current.latestLogsRevision)) {
    return applicationFor(registry, incoming.projectId, incoming.serviceId, false);
  }

  const clearPending = current.clearPending
    ? {
        ...current.clearPending,
        bufferedAppends: current.clearPending.bufferedAppends.filter(
          (event) => event.logsRevision > incoming.logsRevision,
        ),
      }
    : null;
  const snapshot = normalizeSnapshot(incoming, false);
  const nextRegistry = withEntry(registry, {
    ...current,
    snapshot,
    latestLogsRevision: snapshot.logsRevision,
    loaded: true,
    clearPending,
  });
  return applicationFor(nextRegistry, incoming.projectId, incoming.serviceId, true);
}

export function applyLogAppendToRegistry(
  registry: LogsRegistry,
  event: ServiceLogEvent,
): LogsRegistryApplication {
  const current =
    logsEntryForService(registry, event.projectId, event.serviceId) ??
    createEntry(event.projectId, event.serviceId);
  if (!isNewerRevision(event.logsRevision, current.latestLogsRevision)) {
    return applicationFor(registry, event.projectId, event.serviceId, false);
  }

  if (current.clearPending) {
    if (
      current.clearPending.bufferedAppends.some(
        (buffered) => buffered.logsRevision === event.logsRevision,
      )
    ) {
      return applicationFor(registry, event.projectId, event.serviceId, false);
    }
    const bufferedAppends = [...current.clearPending.bufferedAppends, event].sort(
      (left, right) => left.logsRevision - right.logsRevision,
    );
    const nextRegistry = withEntry(registry, {
      ...current,
      clearPending: { ...current.clearPending, bufferedAppends },
    });
    return applicationFor(nextRegistry, event.projectId, event.serviceId, true);
  }

  const nextRegistry = withEntry(registry, appendToCommittedEntry(current, event));
  return applicationFor(nextRegistry, event.projectId, event.serviceId, true);
}

export function beginClearInRegistry(
  registry: LogsRegistry,
  projectId: string,
  serviceId: string,
): BeginLogsClearApplication {
  const current =
    logsEntryForService(registry, projectId, serviceId) ?? createEntry(projectId, serviceId);
  if (current.clearPending) {
    return { ...applicationFor(registry, projectId, serviceId, false), clearRequestId: null };
  }

  const clearRequestId = registry.nextClearRequestId;
  const nextRegistry: LogsRegistry = {
    entries: {
      ...registry.entries,
      [logsRegistryKey(projectId, serviceId)]: {
        ...current,
        clearPending: {
          requestId: clearRequestId,
          baseRevision: current.latestLogsRevision,
          baseRunId: current.snapshot?.runId ?? null,
          bufferedAppends: [],
        },
      },
    },
    nextClearRequestId: clearRequestId + 1,
  };
  return { ...applicationFor(nextRegistry, projectId, serviceId, true), clearRequestId };
}

export function resolveClearInRegistry(
  registry: LogsRegistry,
  incoming: ServiceLogsSnapshot,
  clearRequestId?: number,
): LogsRegistryApplication {
  const current = logsEntryForService(registry, incoming.projectId, incoming.serviceId);

  if (current?.clearPending) {
    const pending = current.clearPending;
    if (clearRequestId !== undefined && clearRequestId !== pending.requestId) {
      return applicationFor(registry, incoming.projectId, incoming.serviceId, false);
    }
    if (!isNewerRevision(incoming.logsRevision, pending.baseRevision)) {
      return applicationFor(registry, incoming.projectId, incoming.serviceId, false);
    }

    let committed: LogsRegistryEntry;
    if (current.latestLogsRevision !== null && current.latestLogsRevision > incoming.logsRevision) {
      committed = { ...current, clearPending: null };
    } else {
      const snapshot = normalizeSnapshot(incoming, true);
      committed = {
        ...current,
        snapshot,
        latestLogsRevision: snapshot.logsRevision,
        loaded: true,
        clearPending: null,
      };
    }
    const replayed = replayAppends(
      committed,
      pending.bufferedAppends.filter((event) => event.logsRevision > incoming.logsRevision),
    );
    const nextRegistry = withEntry(registry, replayed);
    return applicationFor(nextRegistry, incoming.projectId, incoming.serviceId, true);
  }

  const base = current ?? createEntry(incoming.projectId, incoming.serviceId);
  if (!isNewerRevision(incoming.logsRevision, base.latestLogsRevision)) {
    return applicationFor(registry, incoming.projectId, incoming.serviceId, false);
  }
  const snapshot = normalizeSnapshot(incoming, true);
  const nextRegistry = withEntry(registry, {
    ...base,
    snapshot,
    latestLogsRevision: snapshot.logsRevision,
    loaded: true,
    clearPending: null,
  });
  return applicationFor(nextRegistry, incoming.projectId, incoming.serviceId, true);
}

export function rejectClearInRegistry(
  registry: LogsRegistry,
  projectId: string,
  serviceId: string,
  clearRequestId: number,
): LogsRegistryApplication {
  const current = logsEntryForService(registry, projectId, serviceId);
  if (!current?.clearPending || current.clearPending.requestId !== clearRequestId) {
    return applicationFor(registry, projectId, serviceId, false);
  }

  const replayed = replayAppends(current, current.clearPending.bufferedAppends);
  const nextRegistry = withEntry(registry, replayed);
  return applicationFor(nextRegistry, projectId, serviceId, true);
}

export function retainLogsRegistryServices(
  registry: LogsRegistry,
  projectId: string,
  serviceIds: ReadonlySet<string>,
): LogsRegistry {
  let changed = false;
  const entries: Record<string, LogsRegistryEntry> = {};
  for (const [key, entry] of Object.entries(registry.entries)) {
    if (entry.projectId === projectId && !serviceIds.has(entry.serviceId)) {
      changed = true;
    } else {
      entries[key] = entry;
    }
  }
  return changed ? { ...registry, entries } : registry;
}
