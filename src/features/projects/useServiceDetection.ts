import { useCallback, useEffect, useRef, useState } from 'react';
import { detectProjectServices, getErrorMessage } from './api';
import {
  createDetectionReview,
  editDetectionDraft,
  resetDetectionDraft,
  selectAllEligible,
  selectNone as selectNoneInReview,
  toggleSelection,
  type DetectionDraft,
  type DetectionDraftPatch,
  type DetectionReviewState,
} from './detectionDraft';
import type { DetectionResult } from './types';

export type ServiceDetectionStatus = 'idle' | 'loading' | 'success' | 'empty' | 'error';

export interface UseServiceDetectionResult {
  readonly isOpen: boolean;
  readonly status: ServiceDetectionStatus;
  readonly isLoading: boolean;
  readonly result: DetectionResult | null;
  readonly drafts: readonly DetectionDraft[];
  readonly selectedIds: ReadonlySet<string>;
  readonly error: string | null;
  open(): void;
  close(): void;
  scan(): Promise<boolean>;
  retry(): Promise<boolean>;
  toggle(stableId: string): void;
  selectAll(): void;
  selectNone(): void;
  edit(stableId: string, patch: DetectionDraftPatch): void;
  resetDraft(stableId: string): void;
}

interface InFlightRequest {
  readonly generation: number;
  readonly projectId: string;
}

const EMPTY_DRAFTS: readonly DetectionDraft[] = [];
const EMPTY_SELECTED_IDS: ReadonlySet<string> = new Set();

export function useServiceDetection(projectId: string): UseServiceDetectionResult {
  const [isOpen, setIsOpen] = useState(false);
  const [status, setStatus] = useState<ServiceDetectionStatus>('idle');
  const [review, setReview] = useState<DetectionReviewState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(false);
  const projectIdRef = useRef(projectId);
  const requestGenerationRef = useRef(0);
  const inFlightRef = useRef<InFlightRequest | null>(null);

  const invalidateRequests = useCallback(() => {
    requestGenerationRef.current += 1;
    inFlightRef.current = null;
  }, []);

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
    setIsOpen(false);
    setStatus('idle');
    setReview(null);
    setError(null);
  }, [invalidateRequests, projectId]);

  const open = useCallback(() => setIsOpen(true), []);

  const close = useCallback(() => {
    invalidateRequests();
    setIsOpen(false);
    setStatus('idle');
    setReview(null);
    setError(null);
  }, [invalidateRequests]);

  const scan = useCallback(async (): Promise<boolean> => {
    if (inFlightRef.current !== null) {
      return false;
    }

    const generation = requestGenerationRef.current + 1;
    requestGenerationRef.current = generation;
    const request = { generation, projectId };
    inFlightRef.current = request;
    setStatus('loading');
    setError(null);

    const isCurrentRequest = (): boolean => {
      return (
        mountedRef.current &&
        projectIdRef.current === request.projectId &&
        requestGenerationRef.current === request.generation &&
        inFlightRef.current?.generation === request.generation &&
        inFlightRef.current?.projectId === request.projectId
      );
    };

    try {
      const result = await detectProjectServices(request.projectId);
      if (!isCurrentRequest()) {
        return false;
      }

      setReview(createDetectionReview(result));
      setStatus(result.suggestions.length === 0 ? 'empty' : 'success');
      return true;
    } catch (scanError) {
      if (!isCurrentRequest()) {
        return false;
      }
      setStatus('error');
      setError(getErrorMessage(scanError));
      return false;
    } finally {
      if (inFlightRef.current?.generation === generation) {
        inFlightRef.current = null;
      }
    }
  }, [projectId]);

  const toggle = useCallback((stableId: string) => {
    setReview((current) => (current ? toggleSelection(current, stableId) : current));
  }, []);

  const selectAll = useCallback(() => {
    setReview((current) => (current ? selectAllEligible(current) : current));
  }, []);

  const selectNone = useCallback(() => {
    setReview((current) => (current ? selectNoneInReview(current) : current));
  }, []);

  const edit = useCallback((stableId: string, patch: DetectionDraftPatch) => {
    setReview((current) => (current ? editDetectionDraft(current, stableId, patch) : current));
  }, []);

  const resetDraft = useCallback((stableId: string) => {
    setReview((current) => (current ? resetDetectionDraft(current, stableId) : current));
  }, []);

  return {
    isOpen,
    status,
    isLoading: status === 'loading',
    result: review?.originalResult ?? null,
    drafts: review?.drafts ?? EMPTY_DRAFTS,
    selectedIds: review?.selectedIds ?? EMPTY_SELECTED_IDS,
    error,
    open,
    close,
    scan,
    retry: scan,
    toggle,
    selectAll,
    selectNone,
    edit,
    resetDraft,
  };
}
