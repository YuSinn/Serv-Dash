import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  clearServiceLogs,
  getErrorMessage,
  getServiceLogs,
  getServiceRuntime,
  getServiceStartPreview,
  SERVICE_LOG_EVENT,
  SERVICE_LOGS_CLEARED_EVENT,
  SERVICE_RUNTIME_EVENT,
  startService,
  stopService,
} from './api';
import {
  applyLogAppendToRegistry,
  applyLogsSnapshotToRegistry,
  beginClearInRegistry,
  createLogsRegistry,
  logsSnapshotForService,
  rejectClearInRegistry,
  resolveClearInRegistry,
  retainLogsRegistryServices,
  type LogsRegistryApplication,
} from './logsReconciliation';
import {
  applyRuntimeToRegistry,
  createRuntimeRegistry,
  isRuntimeForService,
  retainRuntimeRegistryServices,
} from './runtimeReconciliation';
import type {
  ServiceDefinition,
  ServiceLogEvent,
  ServiceLogsSnapshot,
  ServiceRuntime,
  ServiceRuntimeController,
  ServiceStartPreview,
} from './types';
import { stoppedRuntime } from './types';

export function useServiceRuntime(
  projectId: string,
  services: ServiceDefinition[],
): ServiceRuntimeController {
  const runtimeRegistryRef = useRef(createRuntimeRegistry());
  const logsRegistryRef = useRef(createLogsRegistry());
  const [runtimes, setRuntimes] = useState<Record<string, ServiceRuntime>>(
    runtimeRegistryRef.current.runtimes,
  );
  const [busyServiceIds, setBusyServiceIds] = useState<ReadonlySet<string>>(() => new Set());
  const [selectedServiceId, setSelectedServiceId] = useState<string | null>(null);
  const [selectedLogs, setSelectedLogs] = useState<ServiceLogsSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const serviceIdsRef = useRef<ReadonlySet<string>>(new Set());
  const selectedServiceIdRef = useRef<string | null>(null);

  useEffect(() => {
    selectedServiceIdRef.current = selectedServiceId;
  }, [selectedServiceId]);

  const applyRuntime = useCallback(
    (runtime: ServiceRuntime) => {
      if (runtime.projectId !== projectId || !serviceIdsRef.current.has(runtime.serviceId)) return;
      const result = applyRuntimeToRegistry(runtimeRegistryRef.current, runtime);
      if (!result.accepted) return;
      runtimeRegistryRef.current = result.registry;
      setRuntimes(result.registry.runtimes);
    },
    [projectId],
  );

  const commitLogsApplication = useCallback(
    (serviceId: string, result: LogsRegistryApplication) => {
      if (!result.accepted) return;
      logsRegistryRef.current = result.registry;
      if (selectedServiceIdRef.current === serviceId) setSelectedLogs(result.visibleSnapshot);
    },
    [],
  );

  const applyLogsSnapshot = useCallback(
    (snapshot: ServiceLogsSnapshot) => {
      if (snapshot.projectId !== projectId || !serviceIdsRef.current.has(snapshot.serviceId))
        return;
      commitLogsApplication(
        snapshot.serviceId,
        applyLogsSnapshotToRegistry(logsRegistryRef.current, snapshot),
      );
    },
    [commitLogsApplication, projectId],
  );

  const applyLogsClear = useCallback(
    (snapshot: ServiceLogsSnapshot) => {
      if (snapshot.projectId !== projectId || !serviceIdsRef.current.has(snapshot.serviceId))
        return;
      commitLogsApplication(
        snapshot.serviceId,
        resolveClearInRegistry(logsRegistryRef.current, snapshot),
      );
    },
    [commitLogsApplication, projectId],
  );

  const applyLogAppend = useCallback(
    (event: ServiceLogEvent) => {
      if (event.projectId !== projectId || !serviceIdsRef.current.has(event.serviceId)) return;
      commitLogsApplication(
        event.serviceId,
        applyLogAppendToRegistry(logsRegistryRef.current, event),
      );
    },
    [commitLogsApplication, projectId],
  );

  useEffect(() => {
    const emptyRuntimeRegistry = createRuntimeRegistry();
    const emptyLogsRegistry = createLogsRegistry();
    runtimeRegistryRef.current = emptyRuntimeRegistry;
    logsRegistryRef.current = emptyLogsRegistry;
    setRuntimes(emptyRuntimeRegistry.runtimes);
    setSelectedLogs(null);
  }, [projectId]);

  useEffect(() => {
    let active = true;
    const unlisteners: UnlistenFn[] = [];

    function subscribe<T>(eventName: string, handler: (payload: T) => void): void {
      void listen<T>(eventName, (event) => handler(event.payload))
        .then((unlisten) => {
          if (active) unlisteners.push(unlisten);
          else unlisten();
        })
        .catch((listenError: unknown) => {
          if (active) {
            setError(
              `Real-time service updates could not be connected. ${getErrorMessage(listenError)}`,
            );
          }
        });
    }

    subscribe<ServiceRuntime>(SERVICE_RUNTIME_EVENT, (runtime) => {
      if (runtime.projectId === projectId) applyRuntime(runtime);
    });
    subscribe<ServiceLogEvent>(SERVICE_LOG_EVENT, (event) => {
      if (event.projectId === projectId) applyLogAppend(event);
    });
    subscribe<ServiceLogsSnapshot>(SERVICE_LOGS_CLEARED_EVENT, (snapshot) => {
      if (snapshot.projectId === projectId) applyLogsClear(snapshot);
    });

    return () => {
      active = false;
      for (const unlisten of unlisteners) unlisten();
    };
  }, [applyLogAppend, applyLogsClear, applyRuntime, projectId]);

  useEffect(() => {
    let active = true;
    const serviceIds = new Set(services.map((service) => service.id));
    serviceIdsRef.current = serviceIds;
    const nextRuntimeRegistry = retainRuntimeRegistryServices(
      runtimeRegistryRef.current,
      serviceIds,
    );
    runtimeRegistryRef.current = nextRuntimeRegistry;
    setRuntimes(nextRuntimeRegistry.runtimes);

    const nextLogsRegistry = retainLogsRegistryServices(
      logsRegistryRef.current,
      projectId,
      serviceIds,
    );
    if (nextLogsRegistry !== logsRegistryRef.current) {
      logsRegistryRef.current = nextLogsRegistry;
      setSelectedLogs(logsSnapshotForService(nextLogsRegistry, projectId, selectedServiceId));
    }

    if (selectedServiceId && !serviceIds.has(selectedServiceId)) {
      selectedServiceIdRef.current = null;
      setSelectedServiceId(null);
      setSelectedLogs(null);
    }

    void Promise.allSettled(
      services.map((service) =>
        getServiceRuntime(projectId, service.id).then((runtime) => ({
          runtime,
          serviceId: service.id,
        })),
      ),
    ).then((results) => {
      if (!active) return;
      let firstError: unknown = null;
      for (const result of results) {
        if (result.status === 'fulfilled') {
          if (isRuntimeForService(result.value.runtime, projectId, result.value.serviceId)) {
            applyRuntime(result.value.runtime);
          }
        } else if (firstError === null) firstError = result.reason;
      }
      if (firstError !== null) setError(getErrorMessage(firstError));
    });

    return () => {
      active = false;
    };
  }, [applyRuntime, projectId, selectedServiceId, services]);

  useEffect(() => {
    selectedServiceIdRef.current = selectedServiceId;
    setSelectedLogs(logsSnapshotForService(logsRegistryRef.current, projectId, selectedServiceId));
    if (!selectedServiceId) return;

    let active = true;
    void getServiceLogs(projectId, selectedServiceId)
      .then((logs) => {
        if (!active) return;
        if (logs.projectId !== projectId || logs.serviceId !== selectedServiceId) {
          throw new Error('The logs response did not match the requested service.');
        }
        applyLogsSnapshot(logs);
      })
      .catch((loadError: unknown) => {
        if (active) setError(getErrorMessage(loadError));
      });
    return () => {
      active = false;
    };
  }, [applyLogsSnapshot, projectId, selectedServiceId]);

  const clearError = useCallback(() => setError(null), []);

  const setBusy = useCallback((serviceId: string, busy: boolean) => {
    setBusyServiceIds((current) => {
      const next = new Set(current);
      if (busy) next.add(serviceId);
      else next.delete(serviceId);
      return next;
    });
  }, []);

  const selectLogs = useCallback(
    (serviceId: string | null) => {
      setError(null);
      selectedServiceIdRef.current = serviceId;
      setSelectedLogs(logsSnapshotForService(logsRegistryRef.current, projectId, serviceId));
      setSelectedServiceId(serviceId);
    },
    [projectId],
  );

  const prepareStart = useCallback(
    async (serviceId: string): Promise<ServiceStartPreview | null> => {
      setBusy(serviceId, true);
      setError(null);
      try {
        return await getServiceStartPreview(projectId, serviceId);
      } catch (previewError) {
        setError(getErrorMessage(previewError));
        return null;
      } finally {
        setBusy(serviceId, false);
      }
    },
    [projectId, setBusy],
  );

  const start = useCallback(
    async (serviceId: string): Promise<boolean> => {
      setBusy(serviceId, true);
      selectLogs(serviceId);
      try {
        const runtime = await startService(projectId, serviceId);
        applyRuntime(runtime);
        return true;
      } catch (startError) {
        setError(getErrorMessage(startError));
        return false;
      } finally {
        setBusy(serviceId, false);
      }
    },
    [applyRuntime, projectId, selectLogs, setBusy],
  );

  const stop = useCallback(
    async (serviceId: string): Promise<boolean> => {
      setBusy(serviceId, true);
      setError(null);
      try {
        const runtime = await stopService(projectId, serviceId);
        applyRuntime(runtime);
        return true;
      } catch (stopError) {
        setError(getErrorMessage(stopError));
        return false;
      } finally {
        setBusy(serviceId, false);
      }
    },
    [applyRuntime, projectId, setBusy],
  );

  const clearLogs = useCallback(
    async (serviceId: string): Promise<boolean> => {
      const begin = beginClearInRegistry(logsRegistryRef.current, projectId, serviceId);
      if (!begin.accepted || begin.clearRequestId === null) return false;
      const clearRequestId = begin.clearRequestId;
      commitLogsApplication(serviceId, begin);
      setBusy(serviceId, true);
      setError(null);
      try {
        const logs = await clearServiceLogs(projectId, serviceId);
        if (logs.projectId !== projectId || logs.serviceId !== serviceId) {
          throw new Error('Clear logs returned a snapshot for a different service.');
        }
        commitLogsApplication(
          serviceId,
          resolveClearInRegistry(logsRegistryRef.current, logs, clearRequestId),
        );
        return true;
      } catch (clearFailure) {
        commitLogsApplication(
          serviceId,
          rejectClearInRegistry(logsRegistryRef.current, projectId, serviceId, clearRequestId),
        );
        setError(getErrorMessage(clearFailure));
        return false;
      } finally {
        setBusy(serviceId, false);
      }
    },
    [commitLogsApplication, projectId, setBusy],
  );

  const runtimeFor = useCallback(
    (serviceId: string) => runtimes[serviceId] ?? stoppedRuntime(projectId, serviceId),
    [projectId, runtimes],
  );

  return {
    runtimes,
    busyServiceIds,
    selectedServiceId,
    selectedLogs,
    error,
    clearError,
    runtimeFor,
    prepareStart,
    start,
    stop,
    selectLogs,
    clearLogs,
  };
}
