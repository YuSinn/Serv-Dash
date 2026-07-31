import { useEffect, useRef } from 'react';
import { DetectionWarnings, ServiceSuggestionRow } from './ServiceSuggestionRow';
import type { UseServiceDetectionResult } from './useServiceDetection';

interface ServiceDetectionDialogProps {
  detection: UseServiceDetectionResult;
  onClose: () => void;
}

export function ServiceDetectionDialog({ detection, onClose }: ServiceDetectionDialogProps) {
  const focusTargetRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  useEffect(() => {
    focusTargetRef.current?.focus();
  }, [detection.status]);

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

  if (detection.status === 'loading') {
    content = (
      <div className="detection-state" role="status" aria-live="polite">
        Scanning the project for services...
      </div>
    );
    actions = (
      <button ref={focusTargetRef} className="secondary-button" type="button" onClick={onClose}>
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
          disabled={detection.isLoading}
          onClick={() => void detection.retry()}
        >
          Retry
        </button>
        <button className="secondary-button" type="button" onClick={onClose}>
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
          onClick={() => void detection.scan()}
        >
          Scan again
        </button>
        <button className="secondary-button" type="button" onClick={onClose}>
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
              onClick={detection.selectAll}
            >
              Select all
            </button>
            <button className="secondary-button" type="button" onClick={detection.selectNone}>
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
              onToggle={detection.toggle}
              onEdit={detection.edit}
              onReset={detection.resetDraft}
            />
          ))}
        </div>
        <p className="detection-unsaved-note">Selections and edits are not saved yet.</p>
      </>
    );
    actions = (
      <>
        <button className="primary-button" type="button" onClick={() => void detection.scan()}>
          Scan again
        </button>
        <button className="secondary-button" type="button" onClick={onClose}>
          Close
        </button>
      </>
    );
  } else {
    content = <div className="detection-state">Preparing service detection...</div>;
    actions = (
      <button ref={focusTargetRef} className="secondary-button" type="button" onClick={onClose}>
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
