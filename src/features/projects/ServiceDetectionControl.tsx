import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { buildDetectionSubmissionPlan } from './detectionSubmission';
import { ServiceDetectionDialog } from './ServiceDetectionDialog';
import './serviceDetection.css';
import type { ServiceDefinition } from './types';
import type { UseDetectedServiceSubmissionResult } from './useDetectedServiceSubmission';
import { useServiceDetection } from './useServiceDetection';

interface ServiceDetectionControlProps {
  projectId: string;
  services: readonly ServiceDefinition[];
  submission: UseDetectedServiceSubmissionResult;
  isBusy: boolean;
}

export type ServiceDetectionPresentationPhase = 'review' | 'confirm' | 'submitted';

export function ServiceDetectionControl({
  projectId,
  services,
  submission,
  isBusy,
}: ServiceDetectionControlProps) {
  const detection = useServiceDetection(projectId);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const confirmationOwnerRef = useRef<object | null>(null);
  const [phase, setPhase] = useState<ServiceDetectionPresentationPhase>('review');
  const plan = useMemo(
    () =>
      buildDetectionSubmissionPlan({
        drafts: detection.drafts,
        selectedIds: detection.selectedIds,
        registeredServices: services,
      }),
    [detection.drafts, detection.selectedIds, services],
  );
  const locked = isBusy || submission.status === 'submitting';

  useEffect(() => {
    confirmationOwnerRef.current = null;
    setPhase('review');
  }, [projectId]);

  const handleDetect = useCallback(() => {
    if (locked) {
      return;
    }
    submission.reset();
    setPhase('review');
    detection.open();
    void detection.scan();
  }, [detection.open, detection.scan, locked, submission]);

  const handleClose = useCallback(() => {
    if (locked) {
      return;
    }
    submission.reset();
    confirmationOwnerRef.current = null;
    setPhase('review');
    detection.close();
    queueMicrotask(() => {
      const trigger = triggerRef.current;
      if (trigger?.isConnected && !trigger.disabled) {
        trigger.focus();
      }
    });
  }, [detection.close, locked, submission]);

  const handleContinue = useCallback(() => {
    if (locked || detection.selectedIds.size === 0) {
      return;
    }
    setPhase('confirm');
  }, [detection.selectedIds, locked]);

  const handleBack = useCallback(() => {
    if (locked) {
      return;
    }
    submission.reset();
    setPhase('review');
  }, [locked, submission]);

  const handleConfirm = useCallback(() => {
    if (locked || phase !== 'confirm' || plan.eligible.length === 0) {
      return;
    }
    if (confirmationOwnerRef.current !== null) {
      return;
    }

    const owner = {};
    confirmationOwnerRef.current = owner;
    setPhase('submitted');
    void submission.submit(plan.eligible).finally(() => {
      if (confirmationOwnerRef.current === owner) {
        confirmationOwnerRef.current = null;
      }
    });
  }, [locked, phase, plan.eligible, submission]);

  const handleScanAgain = useCallback(() => {
    if (locked) {
      return;
    }
    submission.reset();
    confirmationOwnerRef.current = null;
    setPhase('review');
    void detection.scan();
  }, [detection.scan, locked, submission]);

  return (
    <>
      <button
        ref={triggerRef}
        className="secondary-button"
        type="button"
        disabled={detection.isLoading || locked}
        aria-busy={detection.isLoading || locked}
        onClick={handleDetect}
      >
        {detection.isLoading ? 'Detecting...' : 'Detect services'}
      </button>
      {detection.isOpen ? (
        <ServiceDetectionDialog
          detection={detection}
          submission={submission}
          phase={phase}
          plan={plan}
          busy={locked}
          onClose={handleClose}
          onContinue={handleContinue}
          onBack={handleBack}
          onConfirm={handleConfirm}
          onScanAgain={handleScanAgain}
        />
      ) : null}
    </>
  );
}
