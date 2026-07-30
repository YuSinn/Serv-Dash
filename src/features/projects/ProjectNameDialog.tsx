import { useEffect, useState, type FormEvent } from 'react';

interface ProjectNameDialogProps {
  title: string;
  description: string;
  rootPath: string;
  initialName: string;
  submitLabel: string;
  error: string | null;
  isBusy: boolean;
  onCancel: () => void;
  onSubmit: (name: string) => void;
}

export function ProjectNameDialog({
  title,
  description,
  rootPath,
  initialName,
  submitLabel,
  error,
  isBusy,
  onCancel,
  onSubmit,
}: ProjectNameDialogProps) {
  const [name, setName] = useState(initialName);

  useEffect(() => {
    setName(initialName);
  }, [initialName]);

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (name.trim()) {
      onSubmit(name);
    }
  }

  return (
    <div className="dialog-backdrop">
      <section
        className="dialog-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="project-name-dialog-title"
        aria-describedby="project-name-dialog-description"
      >
        <div className="dialog-heading">
          <p className="dialog-eyebrow">Project details</p>
          <h2 id="project-name-dialog-title">{title}</h2>
          <p id="project-name-dialog-description">{description}</p>
        </div>

        <form onSubmit={handleSubmit}>
          <label className="form-field">
            <span>Project name</span>
            <input
              autoFocus
              type="text"
              value={name}
              disabled={isBusy}
              onChange={(event) => setName(event.target.value)}
              aria-invalid={Boolean(error)}
            />
          </label>

          <div className="selected-path">
            <span>Root folder</span>
            <strong title={rootPath}>{rootPath}</strong>
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
            <button className="primary-button" type="submit" disabled={isBusy || !name.trim()}>
              {isBusy ? 'Saving...' : submitLabel}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
