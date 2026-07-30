import { useCallback, useEffect, useState } from 'react';
import {
  addService as addServiceRequest,
  getErrorMessage,
  listServices,
  removeService as removeServiceRequest,
  resolveServiceDirectory,
  updateService as updateServiceRequest,
} from './api';
import type { ServiceDefinition, ServiceInput, ServicesController } from './types';

export function useServices(projectId: string): ServicesController {
  const [services, setServices] = useState<ServiceDefinition[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isMutating, setIsMutating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let isActive = true;
    setIsLoading(true);
    setError(null);

    void listServices(projectId)
      .then((loadedServices) => {
        if (isActive) {
          setServices(loadedServices);
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
  }, [projectId]);

  const clearError = useCallback(() => setError(null), []);

  const mutateServices = useCallback(
    async (operation: () => Promise<ServiceDefinition[]>): Promise<boolean> => {
      setIsMutating(true);
      setError(null);
      try {
        setServices(await operation());
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
      setIsMutating(true);
      setError(null);
      try {
        return await resolveServiceDirectory(projectId, selectedPath);
      } catch (resolveError) {
        setError(getErrorMessage(resolveError));
        return null;
      } finally {
        setIsMutating(false);
      }
    },
    [projectId],
  );

  return {
    services,
    isLoading,
    isMutating,
    error,
    clearError,
    addService,
    updateService,
    removeService,
    resolveWorkingDirectory,
  };
}
