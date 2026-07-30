import type {
  ServiceLogEntry,
  ServiceLogEvent,
  ServiceLogsSnapshot,
  ServiceRuntime,
} from './types';

const MAX_VISIBLE_LOG_ENTRIES = 2_000;
const MAX_VISIBLE_LOG_CHARACTERS = 2 * 1024 * 1024;

export interface RuntimeRegistry {
  runtimes: Record<string, ServiceRuntime>;
  latestRuntimeRevision: Record<string, number>;
}

export interface RuntimeRegistryApplication {
  registry: RuntimeRegistry;
  accepted: boolean;
}

export function createRuntimeRegistry(): RuntimeRegistry {
  return {
    runtimes: {},
    latestRuntimeRevision: {},
  };
}

export function limitEntries(entries: ServiceLogEntry[]): ServiceLogEntry[] {
  const limited = entries.slice(-MAX_VISIBLE_LOG_ENTRIES);
  let characters = limited.reduce((total, entry) => total + entry.text.length, 0);
  let first = 0;
  while (characters > MAX_VISIBLE_LOG_CHARACTERS && first < limited.length) {
    characters -= limited[first].text.length;
    first += 1;
  }
  return first === 0 ? limited : limited.slice(first);
}

export function isRuntimeForService(
  runtime: ServiceRuntime,
  projectId: string,
  serviceId: string,
): boolean {
  return runtime.projectId === projectId && runtime.serviceId === serviceId;
}

export function shouldAcceptRuntime(
  current: ServiceRuntime | undefined,
  incoming: ServiceRuntime,
  latestRevision: number,
): boolean {
  void current;
  return incoming.runtimeRevision > latestRevision;
}

export function reconcileRuntime(
  current: ServiceRuntime | undefined,
  incoming: ServiceRuntime,
  latestRevision = current?.runtimeRevision ?? 0,
): ServiceRuntime | undefined {
  return shouldAcceptRuntime(current, incoming, latestRevision) ? incoming : current;
}

export function applyRuntimeToRegistry(
  registry: RuntimeRegistry,
  incoming: ServiceRuntime,
): RuntimeRegistryApplication {
  const current = registry.runtimes[incoming.serviceId];
  const latestRevision = registry.latestRuntimeRevision[incoming.serviceId] ?? 0;
  const nextRuntime = reconcileRuntime(current, incoming, latestRevision);
  if (nextRuntime === current || nextRuntime === undefined) {
    return { registry, accepted: false };
  }

  return {
    accepted: true,
    registry: {
      runtimes: { ...registry.runtimes, [incoming.serviceId]: nextRuntime },
      latestRuntimeRevision: {
        ...registry.latestRuntimeRevision,
        [incoming.serviceId]: nextRuntime.runtimeRevision,
      },
    },
  };
}

export function retainRuntimeRegistryServices(
  registry: RuntimeRegistry,
  serviceIds: ReadonlySet<string>,
): RuntimeRegistry {
  const runtimeIds = Object.keys(registry.runtimes);
  const revisionIds = Object.keys(registry.latestRuntimeRevision);
  if (
    runtimeIds.every((serviceId) => serviceIds.has(serviceId)) &&
    revisionIds.every((serviceId) => serviceIds.has(serviceId))
  ) {
    return registry;
  }

  const runtimes: Record<string, ServiceRuntime> = {};
  for (const serviceId of runtimeIds) {
    if (serviceIds.has(serviceId)) {
      runtimes[serviceId] = registry.runtimes[serviceId];
    }
  }

  const latestRuntimeRevision: Record<string, number> = {};
  for (const serviceId of revisionIds) {
    if (serviceIds.has(serviceId)) {
      latestRuntimeRevision[serviceId] = registry.latestRuntimeRevision[serviceId];
    }
  }

  return { runtimes, latestRuntimeRevision };
}

export function canStopRuntime(runtime: ServiceRuntime): boolean {
  return runtime.status === 'running' || (runtime.status === 'starting' && runtime.pid !== null);
}

export function reconcileLogsSnapshot(
  current: ServiceLogsSnapshot | null,
  incoming: ServiceLogsSnapshot,
  latestRevision = current?.logsRevision ?? 0,
): ServiceLogsSnapshot | null {
  if (incoming.logsRevision <= latestRevision) {
    return current;
  }
  return { ...incoming, entries: limitEntries(incoming.entries) };
}

export function reconcileLogAppend(
  current: ServiceLogsSnapshot | null,
  event: ServiceLogEvent,
  latestRevision = current?.logsRevision ?? 0,
): ServiceLogsSnapshot | null {
  if (event.logsRevision <= latestRevision) {
    return current;
  }
  if (
    current !== null &&
    current.projectId === event.projectId &&
    current.serviceId === event.serviceId &&
    current.runId !== null &&
    current.runId !== event.runId
  ) {
    return current;
  }
  const baseEntries =
    current !== null &&
    current.projectId === event.projectId &&
    current.serviceId === event.serviceId &&
    current.runId === event.runId
      ? current.entries
      : [];
  return {
    projectId: event.projectId,
    serviceId: event.serviceId,
    runId: event.runId,
    logsRevision: event.logsRevision,
    entries: limitEntries([...baseEntries, event.entry]),
  };
}

export function reconcileLogsClear(
  current: ServiceLogsSnapshot | null,
  incoming: ServiceLogsSnapshot,
  latestRevision = current?.logsRevision ?? 0,
): ServiceLogsSnapshot | null {
  if (incoming.logsRevision <= latestRevision) {
    return current;
  }
  return { ...incoming, entries: [] };
}
