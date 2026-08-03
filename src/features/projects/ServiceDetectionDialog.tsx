import { useEffect, useRef } from 'react';
import { getErrorMessage } from './api';
import type { DetectionSubmissionPlan } from './detectionSubmission';
import type { ServiceDetectionPresentationPhase } from './ServiceDetectionControl';
import { DetectionWarnings, ServiceSuggestionRow } from './ServiceSuggestionRow';
import type { UseDetectedServiceSubmissionResult } from './useDetectedServiceSubmission';
import type { UseServiceDetectionResult } from './useServiceDetection';

interface ServiceDetectionDialogProps {
  detection: UseServiceDetectionResult;
  submission: UseDetectedServiceSubmissionResult;
  phase: ServiceDetectionPresentationPhase;
  plan: DetectionSubmissionPlan;
  busy: boolean;
  onClose: () => void;
  onContinue: () => void;
  onBack: () => void;
  onConfirm: () => void;
  onScanAgain: () => void;
}

export function ServiceDetectionDialog({
  detection,
  submission,
  phase,
  plan,
  busy,
  onClose,
  onContinue,
  onBack,
  onConfirm,
  onScanAgain,
}: ServiceDetectionDialogProps) {
  const focusTargetRef = useRef<HTMLButtonElement>(null);
  const locked = busy || submission.status === 'submitting';

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape' && !locked) {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [locked, onClose]);

  useEffect(() => {
    focusTargetRef.current?.focus();
  }, [detection.status, phase, submission.status]);

  const resultDetails = detection.result ? (
    <div className="detection-scan-details">
      {detection.result.truncated ? (
        <p className="detection-truncated-banner">
          The scan was incomplete. Some directories or files may not have been inspected.
        </p>
      ) : null}
      <p>Scanned directories: {detection.result.scannedDirectories}</p>
      <DetectionWarnings warnings={detection.result.warnings} label="Scan warnings" />
    </div>
  ) : null;

  let content;
  let actions;

  if (phase === 'submitted') {
    if (submission.status === 'submitting') {
      content = (
        <div className="detection-state" role="status" aria-live="polite">
          Adding detected services...
        </div>
      );
      actions = null;
    } else if (submission.status === 'success' && submission.result) {
      content = (
        <div className="detection-state">
          <h3>Service import complete</h3>
          <p>{submission.result.added.length} services added.</p>
          <p>{submission.result.skipped.length} services skipped by the server.</p>
          {submission.result.added.length > 0 ? (
            <ul aria-label="Added services">
              {submission.result.added.map((item) => (
                <li key={item.stableId}>{item.service.name}</li>
              ))}
            </ul>
          ) : null}
          {submission.result.skipped.length > 0 ? (
            <ul aria-label="Server-skipped services">
              {submission.result.skipped.map((item) => (
                <li key={item.stableId}>
                  <strong>{item.name}</strong>: {item.message}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      );
      actions = (
        <>
          <button className="primary-button" type="button" disabled={locked} onClick={onScanAgain}>
            Scan again
          </button>
          <button
            ref={focusTargetRef}
            className="secondary-button"
            type="button"
            disabled={locked}
            onClick={onClose}
          >
            Close
          </button>
        </>
      );
    } else if (submission.status === 'error') {
      content = (
        <div className="detection-state detection-error" role="alert">
          {getErrorMessage(submission.error)}
        </div>
      );
      actions = (
        <>
          <button
            ref={focusTargetRef}
            className="primary-button"
            type="button"
            disabled={locked}
            onClick={onBack}
          >
            Back to review
          </button>
          <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
            Close
          </button>
        </>
      );
    } else {
      content = <div className="detection-state">Preparing service submission...</div>;
      actions = null;
    }
  } else if (phase === 'confirm' && detection.status === 'success') {
    content = (
      <div className="detection-state">
        <h3>Confirm detected services</h3>
        <p>{plan.eligible.length} services eligible to add.</p>
        <p>{plan.skipped.length} selected services omitted locally.</p>
        <p>{plan.missingSelectedIds.length} selected services no longer present.</p>
        <p>Only eligible services will be sent.</p>
      </div>
    );
    actions = (
      <>
        <button className="secondary-button" type="button" disabled={locked} onClick={onBack}>
          Back
        </button>
        <button
          ref={focusTargetRef}
          className="primary-button"
          type="button"
          disabled={locked || plan.eligible.length === 0}
          onClick={onConfirm}
        >
          Add {plan.eligible.length} {plan.eligible.length === 1 ? 'service' : 'services'}
        </button>
        <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
          Close
        </button>
      </>
    );
  } else if (detection.status === 'loading') {
    content = (
      <div className="detection-state" role="status" aria-live="polite">
        Scanning the project for services...
      </div>
    );
    actions = (
      <button
        ref={focusTargetRef}
        className="secondary-button"
        type="button"
        disabled={locked}
        onClick={onClose}
      >
        Close
      </button>
    );
  } else if (detection.status === 'error') {
    content = (
      <div className="detection-state detection-error" role="alert">
        {detection.error}
      </div>
    );
    actions = (
      <>
        <button
          ref={focusTargetRef}
          className="primary-button"
          type="button"
          disabled={locked || detection.isLoading}
          onClick={() => {
            if (!locked) {
              void detection.retry();
            }
          }}
        >
          Retry
        </button>
        <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
          Close
        </button>
      </>
    );
  } else if (detection.status === 'empty') {
    content = (
      <div className="detection-state">
        <p>No services were detected in this project.</p>
        {resultDetails}
      </div>
    );
    actions = (
      <>
        <button
          ref={focusTargetRef}
          className="primary-button"
          type="button"
          disabled={locked}
          onClick={onScanAgain}
        >
          Scan again
        </button>
        <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
          Close
        </button>
      </>
    );
  } else if (detection.status === 'success') {
    content = (
      <>
        <div className="detection-review-toolbar">
          <strong>
            {detection.selectedIds.size} of {detection.drafts.length} selected
          </strong>
          <div>
            <button
              ref={focusTargetRef}
              className="secondary-button"
              type="button"
              disabled={locked}
              onClick={() => {
                if (!locked) {
                  detection.selectAll();
                }
              }}
            >
              Select all
            </button>
            <button
              className="secondary-button"
              type="button"
              disabled={locked}
              onClick={() => {
                if (!locked) {
                  detection.selectNone();
                }
              }}
            >
              Select none
            </button>
          </div>
        </div>
        {resultDetails}
        <div className="detection-suggestion-list" role="list" aria-label="Detected services">
          {detection.drafts.map((draft) => (
            <ServiceSuggestionRow
              key={draft.stableId}
              draft={draft}
              selected={detection.selectedIds.has(draft.stableId)}
              disabled={locked}
              onToggle={(stableId) => {
                if (!locked) {
                  detection.toggle(stableId);
                }
              }}
              onEdit={(stableId, patch) => {
                if (!locked) {
                  detection.edit(stableId, patch);
                }
              }}
              onReset={(stableId) => {
                if (!locked) {
                  detection.resetDraft(stableId);
                }
              }}
            />
          ))}
        </div>
        <p className="detection-unsaved-note">Selections and edits are not saved yet.</p>
      </>
    );
    actions = (
      <>
        <button
          className="primary-button"
          type="button"
          disabled={locked || detection.selectedIds.size === 0}
          onClick={onContinue}
        >
          Continue
        </button>
        <button className="secondary-button" type="button" disabled={locked} onClick={onScanAgain}>
          Scan again
        </button>
        <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
          Close
        </button>
      </>
    );
  } else {
    content = <div className="detection-state">Preparing service detection...</div>;
    actions = (
      <button
        ref={focusTargetRef}
        className="secondary-button"
        type="button"
        disabled={locked}
        onClick={onClose}
      >
        Close
      </button>
    );
  }

  return (
    <div className="dialog-backdrop">
      <section
        className="dialog-panel service-detection-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="service-detection-title"
        aria-describedby="service-detection-description"
      >
        <div className="dialog-heading detection-dialog-heading">
          <div>
            <p className="eyebrow">Read-only detection</p>
            <h2 id="service-detection-title">Detected services</h2>
            <p id="service-detection-description">
              Review the detected service suggestions. Closing this dialog discards this review.
            </p>
          </div>
        </div>

        <div className="detection-dialog-body">{content}</div>
        <div className="dialog-actions detection-dialog-actions">{actions}</div>
      </section>
    </div>
  );
}
