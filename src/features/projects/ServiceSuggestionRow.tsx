import { memo, useId } from 'react';
import {
  getDetectionWarningSeverity,
  validateDetectionDraft,
  type DetectionDraft,
  type DetectionDraftPatch,
} from './detectionDraft';
import type { DetectionSourceKind, DetectionWarning } from './types';

const SOURCE_KIND_LABELS: Record<DetectionSourceKind, string> = {
  npmScript: 'npm',
  powerShell: 'PowerShell',
  cmd: 'CMD',
  bat: 'BAT',
};

interface DetectionWarningsProps {
  warnings: readonly DetectionWarning[];
  label: string;
}

export function DetectionWarnings({ warnings, label }: DetectionWarningsProps) {
  if (warnings.length === 0) {
    return null;
  }

  return (
    <ul className="detection-warning-list" aria-label={label}>
      {warnings.map((warning, index) => {
        const severity = getDetectionWarningSeverity(warning.kind);
        return (
          <li
            className={`detection-warning detection-warning--${severity}`}
            key={`${warning.kind}-${index}`}
          >
            <strong>{severity}</strong>
            <span>{warning.message}</span>
            {warning.path ? <code>{warning.path}</code> : null}
          </li>
        );
      })}
    </ul>
  );
}

interface ServiceSuggestionRowProps {
  draft: DetectionDraft;
  selected: boolean;
  onToggle: (stableId: string) => void;
  onEdit: (stableId: string, patch: DetectionDraftPatch) => void;
  onReset: (stableId: string) => void;
}

export const ServiceSuggestionRow = memo(function ServiceSuggestionRow({
  draft,
  selected,
  onToggle,
  onEdit,
  onReset,
}: ServiceSuggestionRowProps) {
  const id = useId();
  const errors = validateDetectionDraft(draft);
  const displayNameErrorId = `${id}-display-name-error`;
  const commandErrorId = `${id}-command-error`;
  const workingDirectoryErrorId = `${id}-working-directory-error`;

  return (
    <article
      className={`detection-suggestion${draft.matchesRegisteredService ? ' detection-suggestion--registered' : ''}`}
      data-testid="service-suggestion"
      role="listitem"
    >
      <div className="detection-suggestion-header">
        <label className="detection-selection" htmlFor={`${id}-selected`}>
          <input
            id={`${id}-selected`}
            type="checkbox"
            checked={selected}
            disabled={draft.matchesRegisteredService}
            onChange={() => onToggle(draft.stableId)}
          />
          <span>Select {draft.displayName}</span>
        </label>
        <span className="detection-source-badge">{SOURCE_KIND_LABELS[draft.sourceKind]}</span>
        {draft.matchesRegisteredService ? (
          <span className="detection-registered-label">Already registered</span>
        ) : null}
      </div>

      <dl className="detection-source-details">
        <div>
          <dt>Source path</dt>
          <dd>{draft.sourcePath}</dd>
        </div>
        <div>
          <dt>Reason</dt>
          <dd>{draft.reason}</dd>
        </div>
      </dl>

      <div className="detection-fields">
        <label className="form-field">
          <span>Display name</span>
          <input
            type="text"
            maxLength={121}
            value={draft.displayName}
            aria-invalid={Boolean(errors.displayName)}
            aria-describedby={errors.displayName ? displayNameErrorId : undefined}
            onChange={(event) => onEdit(draft.stableId, { displayName: event.target.value })}
          />
          {errors.displayName ? (
            <span className="field-error" id={displayNameErrorId}>
              {errors.displayName}
            </span>
          ) : null}
        </label>

        <label className="form-field">
          <span>Working directory</span>
          <input
            type="text"
            value={draft.workingDirectory}
            spellCheck={false}
            aria-invalid={Boolean(errors.workingDirectory)}
            aria-describedby={errors.workingDirectory ? workingDirectoryErrorId : undefined}
            onChange={(event) => onEdit(draft.stableId, { workingDirectory: event.target.value })}
          />
          {errors.workingDirectory ? (
            <span className="field-error" id={workingDirectoryErrorId}>
              {errors.workingDirectory}
            </span>
          ) : null}
        </label>

        <label className="form-field detection-command-field">
          <span>Command</span>
          <textarea
            rows={2}
            maxLength={4097}
            value={draft.command}
            spellCheck={false}
            aria-invalid={Boolean(errors.command)}
            aria-describedby={errors.command ? commandErrorId : undefined}
            onChange={(event) => onEdit(draft.stableId, { command: event.target.value })}
          />
          {errors.command ? (
            <span className="field-error" id={commandErrorId}>
              {errors.command}
            </span>
          ) : null}
        </label>
      </div>

      <DetectionWarnings warnings={draft.warnings} label={`Warnings for ${draft.displayName}`} />

      <button
        className="secondary-button detection-reset-button"
        type="button"
        onClick={() => onReset(draft.stableId)}
      >
        Reset
      </button>
    </article>
  );
});
