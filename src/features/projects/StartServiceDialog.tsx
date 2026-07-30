import type { ServiceStartPreview } from './types';

interface StartServiceDialogProps {
  preview: ServiceStartPreview;
  error: string | null;
  isBusy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function StartServiceDialog({
  preview,
  error,
  isBusy,
  onCancel,
  onConfirm,
}: StartServiceDialogProps) {
  return (
    <div className="dialog-backdrop">
      <section
        className="dialog-panel start-service-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="start-service-dialog-title"
        aria-describedby="start-service-dialog-description"
      >
        <div className="dialog-heading">
          <p className="dialog-eyebrow dialog-eyebrow-runtime">Run a local command</p>
          <h2 id="start-service-dialog-title">Start {preview.serviceName}?</h2>
          <p id="start-service-dialog-description">
            Server Dashboard will run this configured shell command locally without administrator
            privileges. Review every value before continuing.
          </p>
        </div>

        <dl className="start-preview-details">
          <div>
            <dt>Project</dt>
            <dd>{preview.projectName}</dd>
          </div>
          <div>
            <dt>Service</dt>
            <dd>{preview.serviceName}</dd>
          </div>
          <div>
            <dt>Resolved working directory</dt>
            <dd title={preview.resolvedWorkingDirectory}>{preview.resolvedWorkingDirectory}</dd>
          </div>
          <div>
            <dt>Configured command</dt>
            <dd>
              <pre>
                <code>{preview.command}</code>
              </pre>
            </dd>
          </div>
        </dl>

        <p className="start-runtime-warning">
          This is a Windows shell command executed through <code>cmd.exe /D /S /C</code>. Stop
          force-terminates its entire managed process tree.
        </p>

        {error ? (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button className="secondary-button" type="button" disabled={isBusy} onClick={onCancel}>
            Cancel
          </button>
          <button className="primary-button" type="button" disabled={isBusy} onClick={onConfirm}>
            {isBusy ? 'Starting...' : 'Run configured command'}
          </button>
        </div>
      </section>
    </div>
  );
}
