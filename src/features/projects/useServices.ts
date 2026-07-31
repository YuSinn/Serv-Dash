import { useCallback, useEffect, useRef, useState } from 'react';
import {
  addService as addServiceRequest,
  getErrorMessage,
  listServices,
  removeService as removeServiceRequest,
  resolveServiceDirectory,
  updateService as updateServiceRequest,
} from './api';
import type { ServiceDefinition, ServiceInput, ServicesController } from './types';
import {
  createServiceMutationCoordinator,
  useDetectedServiceSubmission,
  type ServiceMutationCoordinator,
  type UseDetectedServiceSubmissionResult,
} from './useDetectedServiceSubmission';

export interface UseServicesResult extends ServicesController {
  readonly detectedSubmission: UseDetectedServiceSubmissionResult;
}

export function useServices(projectId: string): UseServicesResult {
  const [services, setServices] = useState<ServiceDefinition[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isManualMutating, setIsManualMutating] = useState(false);
  const [isResolvingDirectory, setIsResolvingDirectory] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(false);
  const projectIdRef = useRef(projectId);
  const manualGenerationRef = useRef(0);
  const mutationCoordinatorRef = useRef<ServiceMutationCoordinator | null>(null);
  mutationCoordinatorRef.current ??= createServiceMutationCoordinator();
  const mutationCoordinator = mutationCoordinatorRef.current;
  const replaceServices = useCallback((nextServices: ServiceDefinition[]) => {
    setServices(nextServices);
  }, []);
  const detectedSubmission = useDetectedServiceSubmission(
    projectId,
    replaceServices,
    mutationCoordinator,
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      manualGenerationRef.current += 1;
      mutationCoordinator.invalidate();
    };
  }, [mutationCoordinator]);

  useEffect(() => {
    let isActive = true;
    projectIdRef.current = projectId;
    manualGenerationRef.current += 1;
    mutationCoordinator.invalidate();
    setServices([]);
    setIsLoading(true);
    setIsManualMutating(false);
    setIsResolvingDirectory(false);
    setError(null);

    const loadOwner = mutationCoordinator.acquire();
    if (loadOwner === null) {
      setIsLoading(false);
      return () => {
        isActive = false;
      };
    }
    const isCurrentLoad = (): boolean =>
      isActive &&
      mountedRef.current &&
      projectIdRef.current === projectId &&
      mutationCoordinator.isOwner(loadOwner);

    void listServices(projectId)
      .then((loadedServices) => {
        if (isCurrentLoad()) {
          setServices(loadedServices);
        }
      })
      .catch((loadError: unknown) => {
        if (isCurrentLoad()) {
          setError(getErrorMessage(loadError));
        }
      })
      .finally(() => {
        if (isCurrentLoad()) {
          setIsLoading(false);
        }
        mutationCoordinator.release(loadOwner);
      });

    return () => {
      isActive = false;
    };
  }, [mutationCoordinator, projectId]);

  const clearError = useCallback(() => setError(null), []);

  const mutateServices = useCallback(
    async (operation: () => Promise<ServiceDefinition[]>): Promise<boolean> => {
      if (!mountedRef.current || projectIdRef.current !== projectId) {
        return false;
      }
      const owner = mutationCoordinator.acquire();
      if (owner === null) {
        return false;
      }
      const generation = manualGenerationRef.current + 1;
      manualGenerationRef.current = generation;
      const isCurrentMutation = (): boolean =>
        mountedRef.current &&
        projectIdRef.current === projectId &&
        manualGenerationRef.current === generation &&
        mutationCoordinator.isOwner(owner);

      setIsManualMutating(true);
      setError(null);
      try {
        const nextServices = await operation();
        if (!isCurrentMutation()) {
          return false;
        }
        setServices(nextServices);
        return true;
      } catch (mutationError) {
        if (!isCurrentMutation()) {
          return false;
        }
        setError(getErrorMessage(mutationError));
        return false;
      } finally {
        if (isCurrentMutation()) {
          setIsManualMutating(false);
        }
        mutationCoordinator.release(owner);
      }
    },
    [mutationCoordinator, projectId],
  );

  const addService = useCallback(
    (service: ServiceInput) => mutateServices(() => addServiceRequest(projectId, service)),
    [mutateServices, projectId],
  );

  const updateService = useCallback(
    (serviceId: string, service: ServiceInput) =>
      mutateServices(() => updateServiceRequest(projectId, serviceId, service)),
    [mutateServices, projectId],
  );

  const removeService = useCallback(
    (serviceId: string) => mutateServices(() => removeServiceRequest(projectId, serviceId)),
    [mutateServices, projectId],
  );

  const resolveWorkingDirectory = useCallback(
    async (selectedPath: string): Promise<string | null> => {
      setIsResolvingDirectory(true);
      setError(null);
      try {
        return await resolveServiceDirectory(projectId, selectedPath);
      } catch (resolveError) {
        setError(getErrorMessage(resolveError));
        return null;
      } finally {
        setIsResolvingDirectory(false);
      }
    },
    [projectId],
  );

  return {
    services,
    isLoading,
    isMutating:
      isManualMutating || isResolvingDirectory || detectedSubmission.status === 'submitting',
    error,
    clearError,
    addService,
    updateService,
    removeService,
    resolveWorkingDirectory,
    detectedSubmission,
  };
}
