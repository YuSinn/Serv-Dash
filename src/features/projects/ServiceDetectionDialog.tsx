import { useEffect, useRef } from 'react';
import { getErrorMessage } from './api';
import type { DetectionSubmissionPlan } from './detectionSubmission';
import type { ServiceDetectionPresentationPhase } from './ServiceDetectionControl';
import { DetectionWarnings, ServiceSuggestionRow } from './ServiceSuggestionRow';
import type { UseDetectedServiceSubmissionResult } from './useDetectedServiceSubmission';
import type { UseServiceDetectionResult } from './useServiceDetection';

const FOCUSABLE_SELECTOR =
  'a[href], button, input:not([type=hidden]), select, textarea, [contenteditable=true], [tabindex]';

function countLabel(count: number, singular: string, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`;
}

function focusableElements(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (element) => {
      const style = window.getComputedStyle(element);
      return (
        element.tabIndex >= 0 &&
        !element.matches(':disabled') &&
        !element.closest('[hidden], [aria-hidden=true]') &&
        style.display !== 'none' &&
        style.visibility !== 'hidden'
      );
    },
  );
}

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
  const dialogRef = useRef<HTMLElement>(null);
  const focusTargetRef = useRef<HTMLElement>(null);
  const locked = busy || submission.status === 'submitting';

  function setFocusTarget(element: HTMLElement | null) {
    focusTargetRef.current = element;
  }

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        if (!locked) {
          onClose();
        }
        return;
      }

      if (event.key !== 'Tab' || !dialogRef.current) {
        return;
      }

      const elements = focusableElements(dialogRef.current);
      const activeElement = document.activeElement;
      if (elements.length === 0) {
        event.preventDefault();
        event.stopPropagation();
        (focusTargetRef.current ?? dialogRef.current).focus();
        return;
      }

      const first = elements[0];
      const last = elements[elements.length - 1];
      const activeIndex =
        activeElement instanceof HTMLElement ? elements.indexOf(activeElement) : -1;
      if (event.shiftKey ? activeIndex <= 0 : activeIndex === -1 || activeElement === last) {
        event.preventDefault();
        event.stopPropagation();
        (event.shiftKey ? last : first)?.focus();
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
        <div
          ref={setFocusTarget}
          className="detection-state"
          role="status"
          aria-live="polite"
          tabIndex={-1}
        >
          Adding detected services...
        </div>
      );
      actions = null;
    } else if (submission.status === 'success' && submission.result) {
      content = (
        <div className="detection-state" role="status">
          <h3 ref={setFocusTarget} tabIndex={-1}>
            Service import complete
          </h3>
          <p>{countLabel(submission.result.added.length, 'service')} added.</p>
          <p>{countLabel(submission.result.skipped.length, 'service')} skipped by the server.</p>
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
          <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
            Close
          </button>
        </>
      );
    } else if (submission.status === 'error') {
      content = (
        <div
          ref={setFocusTarget}
          className="detection-state detection-error"
          role="alert"
          tabIndex={-1}
        >
          {getErrorMessage(submission.error)}
        </div>
      );
      actions = (
        <>
          <button className="primary-button" type="button" disabled={locked} onClick={onBack}>
            Back to review
          </button>
          <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
            Close
          </button>
        </>
      );
    } else {
      content = (
        <div
          ref={setFocusTarget}
          className="detection-state"
          role="status"
          aria-live="polite"
          tabIndex={-1}
        >
          Preparing service submission...
        </div>
      );
      actions = null;
    }
  } else if (phase === 'confirm' && detection.status === 'success') {
    content = (
      <div className="detection-state">
        <h3 ref={setFocusTarget} tabIndex={-1}>
          Confirm detected services
        </h3>
        <p>{countLabel(plan.eligible.length, 'service')} eligible to add.</p>
        <p>{countLabel(plan.skipped.length, 'selected service')} omitted locally.</p>
        <p>{countLabel(plan.missingSelectedIds.length, 'selection')} no longer present.</p>
        <p>Only eligible services will be sent.</p>
      </div>
    );
    actions = (
      <>
        <button className="secondary-button" type="button" disabled={locked} onClick={onBack}>
          Back
        </button>
        <button
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
      <div
        ref={setFocusTarget}
        className="detection-state"
        role="status"
        aria-live="polite"
        tabIndex={-1}
      >
        Scanning the project for services...
      </div>
    );
    actions = (
      <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
        Close
      </button>
    );
  } else if (detection.status === 'error') {
    content = (
      <div
        ref={setFocusTarget}
        className="detection-state detection-error"
        role="alert"
        tabIndex={-1}
      >
        {detection.error}
      </div>
    );
    actions = (
      <>
        <button
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
      <div ref={setFocusTarget} className="detection-state" tabIndex={-1}>
        <p>No services were detected in this project.</p>
        {resultDetails}
      </div>
    );
    actions = (
      <>
        <button className="primary-button" type="button" disabled={locked} onClick={onScanAgain}>
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
          <strong ref={setFocusTarget} tabIndex={-1}>
            {detection.selectedIds.size} of {detection.drafts.length} selected
          </strong>
          <div>
            <button
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
    content = (
      <div
        ref={setFocusTarget}
        className="detection-state"
        role="status"
        aria-live="polite"
        tabIndex={-1}
      >
        Preparing service detection...
      </div>
    );
    actions = (
      <button className="secondary-button" type="button" disabled={locked} onClick={onClose}>
        Close
      </button>
    );
  }

  return (
    <div className="dialog-backdrop">
      <section
        ref={dialogRef}
        className="dialog-panel service-detection-dialog"
        role="dialog"
        aria-modal="true"
        aria-busy={locked || detection.isLoading}
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
