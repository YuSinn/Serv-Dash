import type { Project } from './types';

interface RemoveProjectDialogProps {
  project: Project;
  error: string | null;
  isBusy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function RemoveProjectDialog({
  project,
  error,
  isBusy,
  onCancel,
  onConfirm,
}: RemoveProjectDialogProps) {
  return (
    <div className="dialog-backdrop">
      <section
        className="dialog-panel dialog-panel-compact"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="remove-dialog-title"
        aria-describedby="remove-dialog-description"
      >
        <div className="dialog-heading">
          <p className="dialog-eyebrow dialog-eyebrow-danger">Remove project</p>
          <h2 id="remove-dialog-title">Remove {project.name}?</h2>
          <p id="remove-dialog-description">
            This removes only the saved record from Server Dashboard. The folder and all of its
            files will not be deleted.
          </p>
        </div>

        <div className="selected-path">
          <span>Folder kept on disk</span>
          <strong title={project.rootPath}>{project.rootPath}</strong>
        </div>

        {error ? (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button className="secondary-button" type="button" disabled={isBusy} onClick={onCancel}>
            Cancel
          </button>
          <button className="danger-button" type="button" disabled={isBusy} onClick={onConfirm}>
            {isBusy ? 'Removing...' : 'Remove from dashboard'}
          </button>
        </div>
      </section>
    </div>
  );
}
