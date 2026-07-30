import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useState } from 'react';
import {
  addProject as addProjectRequest,
  getErrorMessage,
  getServiceRuntime,
  listProjects,
  openProjectFolder as openProjectFolderRequest,
  removeProject as removeProjectRequest,
  renameProject as renameProjectRequest,
  SERVICE_RUNTIME_EVENT,
} from './api';
import type { Project, ProjectsController, ServiceRuntime, ServiceRuntimeStatus } from './types';
import { isRuntimeActive } from './types';

function runtimeKey(projectId: string, serviceId: string): string {
  return `${projectId}:${serviceId}`;
}

export function useProjects(): ProjectsController {
  const [projects, setProjects] = useState<Project[]>([]);
  const [runtimeStatuses, setRuntimeStatuses] = useState<Record<string, ServiceRuntimeStatus>>({});
  const [isLoading, setIsLoading] = useState(true);
  const [isMutating, setIsMutating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | null = null;
    void listen<ServiceRuntime>(SERVICE_RUNTIME_EVENT, (event) => {
      const runtime = event.payload;
      setRuntimeStatuses((current) => ({
        ...current,
        [runtimeKey(runtime.projectId, runtime.serviceId)]: runtime.status,
      }));
    })
      .then((stopListening) => {
        if (active) {
          unlisten = stopListening;
        } else {
          stopListening();
        }
      })
      .catch((listenError: unknown) => {
        if (active) {
          setError(
            `Project runtime protection updates could not be connected. ${getErrorMessage(listenError)}`,
          );
        }
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let isActive = true;

    void listProjects()
      .then(async (loadedProjects) => {
        if (!isActive) {
          return;
        }
        setProjects(loadedProjects);
        const results = await Promise.allSettled(
          loadedProjects.flatMap((project) =>
            project.services.map((service) => getServiceRuntime(project.id, service.id)),
          ),
        );
        if (!isActive) {
          return;
        }
        let firstError: unknown = null;
        const loadedStatuses: Record<string, ServiceRuntimeStatus> = {};
        for (const result of results) {
          if (result.status === 'fulfilled') {
            loadedStatuses[runtimeKey(result.value.projectId, result.value.serviceId)] =
              result.value.status;
          } else if (firstError === null) {
            firstError = result.reason;
          }
        }
        setRuntimeStatuses((current) => ({ ...current, ...loadedStatuses }));
        if (firstError !== null) {
          setError(getErrorMessage(firstError));
        }
      })
      .catch((loadError: unknown) => {
        if (isActive) {
          setError(getErrorMessage(loadError));
        }
      })
      .finally(() => {
        if (isActive) {
          setIsLoading(false);
        }
      });

    return () => {
      isActive = false;
    };
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const mutateProjects = useCallback(
    async (operation: () => Promise<Project[]>): Promise<boolean> => {
      setIsMutating(true);
      setError(null);
      try {
        setProjects(await operation());
        return true;
      } catch (mutationError) {
        setError(getErrorMessage(mutationError));
        return false;
      } finally {
        setIsMutating(false);
      }
    },
    [],
  );

  const addProject = useCallback(
    (rootPath: string, name: string) => mutateProjects(() => addProjectRequest(rootPath, name)),
    [mutateProjects],
  );

  const renameProject = useCallback(
    (projectId: string, name: string) =>
      mutateProjects(() => renameProjectRequest(projectId, name)),
    [mutateProjects],
  );

  const removeProject = useCallback(
    (projectId: string) => mutateProjects(() => removeProjectRequest(projectId)),
    [mutateProjects],
  );

  const openProjectFolder = useCallback(async (projectId: string): Promise<boolean> => {
    setIsMutating(true);
    setError(null);
    try {
      await openProjectFolderRequest(projectId);
      return true;
    } catch (openError) {
      setError(getErrorMessage(openError));
      return false;
    } finally {
      setIsMutating(false);
    }
  }, []);

  const isProjectActive = useCallback(
    (projectId: string): boolean => {
      const prefix = `${projectId}:`;
      return Object.entries(runtimeStatuses).some(
        ([key, status]) => key.startsWith(prefix) && isRuntimeActive(status),
      );
    },
    [runtimeStatuses],
  );

  return {
    projects,
    isLoading,
    isMutating,
    error,
    clearError,
    isProjectActive,
    addProject,
    renameProject,
    removeProject,
    openProjectFolder,
  };
}
