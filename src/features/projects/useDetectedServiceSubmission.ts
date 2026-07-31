import { useCallback, useEffect, useRef, useState } from 'react';
import { addDetectedServices } from './api';
import {
  preparedDetectedServicesToSubmissions,
  type PreparedDetectedService,
} from './detectionSubmission';
import type { AddDetectedServicesResult, ServiceDefinition } from './types';

export type DetectedServiceSubmissionStatus = 'idle' | 'submitting' | 'success' | 'error';

export interface UseDetectedServiceSubmissionResult {
  readonly status: DetectedServiceSubmissionStatus;
  readonly result: AddDetectedServicesResult | null;
  readonly error: unknown | null;
  submit(prepared: readonly PreparedDetectedService[]): Promise<boolean>;
  reset(): void;
}

type ServiceMutationOwner = object;

export interface ServiceMutationCoordinator {
  acquire(): ServiceMutationOwner | null;
  isOwner(owner: ServiceMutationOwner): boolean;
  release(owner: ServiceMutationOwner): void;
  invalidate(): void;
}

export function createServiceMutationCoordinator(): ServiceMutationCoordinator {
  let owner: ServiceMutationOwner | null = null;

  return {
    acquire() {
      if (owner !== null) {
        return null;
      }
      owner = {};
      return owner;
    },
    isOwner(candidate) {
      return owner === candidate;
    },
    release(candidate) {
      if (owner === candidate) {
        owner = null;
      }
    },
    invalidate() {
      owner = null;
    },
  };
}

interface InFlightRequest {
  readonly generation: number;
  readonly projectId: string;
  readonly owner: ServiceMutationOwner;
}

export function useDetectedServiceSubmission(
  projectId: string,
  replaceServices: (services: ServiceDefinition[]) => void,
  mutationCoordinator: ServiceMutationCoordinator,
): UseDetectedServiceSubmissionResult {
  const [status, setStatus] = useState<DetectedServiceSubmissionStatus>('idle');
  const [result, setResult] = useState<AddDetectedServicesResult | null>(null);
  const [error, setError] = useState<unknown | null>(null);
  const mountedRef = useRef(false);
  const projectIdRef = useRef(projectId);
  const requestGenerationRef = useRef(0);
  const inFlightRef = useRef<InFlightRequest | null>(null);

  const invalidateRequests = useCallback(() => {
    requestGenerationRef.current += 1;
    inFlightRef.current = null;
    mutationCoordinator.invalidate();
  }, [mutationCoordinator]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      invalidateRequests();
    };
  }, [invalidateRequests]);

  useEffect(() => {
    projectIdRef.current = projectId;
    invalidateRequests();
    setStatus('idle');
    setResult(null);
    setError(null);
  }, [invalidateRequests, projectId]);

  const submit = useCallback(
    async (prepared: readonly PreparedDetectedService[]): Promise<boolean> => {
      if (
        !mountedRef.current ||
        projectIdRef.current !== projectId ||
        inFlightRef.current !== null
      ) {
        return false;
      }
      if (prepared.length === 0) {
        setStatus('idle');
        setResult(null);
        setError(null);
        return false;
      }

      const owner = mutationCoordinator.acquire();
      if (owner === null) {
        return false;
      }
      const generation = requestGenerationRef.current + 1;
      requestGenerationRef.current = generation;
      const request = { generation, projectId, owner };
      inFlightRef.current = request;
      setStatus('submitting');
      setResult(null);
      setError(null);

      const isCurrentRequest = (): boolean =>
        mountedRef.current &&
        projectIdRef.current === request.projectId &&
        requestGenerationRef.current === request.generation &&
        inFlightRef.current === request &&
        mutationCoordinator.isOwner(request.owner);

      try {
        const submissions = preparedDetectedServicesToSubmissions(prepared);
        const response = await addDetectedServices(request.projectId, submissions);
        if (!isCurrentRequest()) {
          return false;
        }

        replaceServices(response.services);
        setResult(response);
        setStatus('success');
        return true;
      } catch (submissionError) {
        if (!isCurrentRequest()) {
          return false;
        }

        setResult(null);
        setError(submissionError);
        setStatus('error');
        return false;
      } finally {
        if (inFlightRef.current === request) {
          inFlightRef.current = null;
        }
        mutationCoordinator.release(request.owner);
      }
    },
    [mutationCoordinator, projectId, replaceServices],
  );

  const reset = useCallback(() => {
    if (inFlightRef.current !== null) {
      return;
    }
    requestGenerationRef.current += 1;
    setStatus('idle');
    setResult(null);
    setError(null);
  }, []);

  return { status, result, error, submit, reset };
}
