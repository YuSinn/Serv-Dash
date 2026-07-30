import type { ServiceDefinition } from './types';

interface RemoveServiceDialogProps {
  service: ServiceDefinition;
  error: string | null;
  isBusy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function RemoveServiceDialog({
  service,
  error,
  isBusy,
  onCancel,
  onConfirm,
}: RemoveServiceDialogProps) {
  return (
    <div className="dialog-backdrop">
      <section
        className="dialog-panel dialog-panel-compact"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="remove-service-dialog-title"
        aria-describedby="remove-service-dialog-description"
      >
        <div className="dialog-heading">
          <p className="dialog-eyebrow dialog-eyebrow-danger">Remove service configuration</p>
          <h2 id="remove-service-dialog-title">Remove {service.name}?</h2>
          <p id="remove-service-dialog-description">
            This removes only the saved service configuration from Server Dashboard. No folder,
            file, script, or command will be changed or deleted.
          </p>
        </div>

        <div className="selected-path">
          <span>Configured working directory kept on disk</span>
          <strong>{service.workingDirectory}</strong>
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
            {isBusy ? 'Removing...' : 'Remove configuration'}
          </button>
        </div>
      </section>
    </div>
  );
}
