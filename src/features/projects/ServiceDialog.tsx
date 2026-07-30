import { open } from '@tauri-apps/plugin-dialog';
import { useMemo, useState, type FormEvent } from 'react';
import { getErrorMessage } from './api';
import type { ServiceDefinition, ServiceInput } from './types';

interface ServiceDialogProps {
  projectRoot: string;
  service: ServiceDefinition | null;
  services: ServiceDefinition[];
  error: string | null;
  isBusy: boolean;
  onCancel: () => void;
  onResolveDirectory: (selectedPath: string) => Promise<string | null>;
  onSubmit: (service: ServiceInput) => void;
}

interface ServiceForm {
  name: string;
  workingDirectory: string;
  command: string;
  expectedPort: string;
  localUrl: string;
}

type FieldName = keyof ServiceForm;
type FieldErrors = Partial<Record<FieldName, string>>;
type TouchedFields = Partial<Record<FieldName, boolean>>;

function initialForm(service: ServiceDefinition | null): ServiceForm {
  return {
    name: service?.name ?? '',
    workingDirectory: service?.workingDirectory ?? '.',
    command: service?.command ?? '',
    expectedPort: service?.expectedPort?.toString() ?? '',
    localUrl: service?.localUrl ?? '',
  };
}

function validateForm(
  form: ServiceForm,
  services: ServiceDefinition[],
  editedServiceId: string | null,
): FieldErrors {
  const errors: FieldErrors = {};
  const name = form.name.trim();
  if (!name) {
    errors.name = 'Service name is required.';
  } else if (name.length > 120) {
    errors.name = 'Use at most 120 characters.';
  } else if (
    services.some(
      (service) =>
        service.id !== editedServiceId &&
        service.name.trim().toLocaleLowerCase() === name.toLocaleLowerCase(),
    )
  ) {
    errors.name = 'A service with this name is already configured.';
  }

  const directory = form.workingDirectory.trim();
  if (!directory) {
    errors.workingDirectory = "Working directory is required. Use '.' for the root.";
  } else if (/^(?:[a-z]:|[\\/])/i.test(directory)) {
    errors.workingDirectory = 'Use a path relative to the project root.';
  } else if (directory.split(/[\\/]+/).includes('..')) {
    errors.workingDirectory = "Parent components ('..') are not allowed.";
  }

  const command = form.command.trim();
  if (!command) {
    errors.command = 'Command is required.';
  } else if (command.length > 4096) {
    errors.command = 'Use at most 4096 characters.';
  } else if (/\0|[\r\n]/.test(command)) {
    errors.command = 'NUL characters and line breaks are not allowed.';
  }

  const portText = form.expectedPort.trim();
  if (portText) {
    const port = Number(portText);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      errors.expectedPort = 'Enter a whole number from 1 to 65535.';
    }
  }

  const urlText = form.localUrl.trim();
  if (urlText) {
    if (!/^https?:\/\//i.test(urlText)) {
      errors.localUrl = 'Use a valid HTTP or HTTPS URL.';
    } else {
      try {
        const url = new URL(urlText);
        if (
          !['http:', 'https:'].includes(url.protocol) ||
          !url.hostname ||
          url.username ||
          url.password
        ) {
          errors.localUrl = 'Use HTTP or HTTPS without embedded credentials.';
        }
      } catch {
        errors.localUrl = 'Use a valid HTTP or HTTPS URL.';
      }
    }
  }

  return errors;
}

export function ServiceDialog({
  projectRoot,
  service,
  services,
  error,
  isBusy,
  onCancel,
  onResolveDirectory,
  onSubmit,
}: ServiceDialogProps) {
  const [form, setForm] = useState<ServiceForm>(() => initialForm(service));
  const [touched, setTouched] = useState<TouchedFields>({});
  const [submitted, setSubmitted] = useState(false);
  const [pickerError, setPickerError] = useState<string | null>(null);
  const errors = useMemo(
    () => validateForm(form, services, service?.id ?? null),
    [form, service?.id, services],
  );

  function setField(field: FieldName, value: string) {
    setForm((current) => ({ ...current, [field]: value }));
  }

  function touch(field: FieldName) {
    setTouched((current) => ({ ...current, [field]: true }));
  }

  function fieldError(field: FieldName): string | undefined {
    return submitted || touched[field] ? errors[field] : undefined;
  }

  async function browseForDirectory() {
    setPickerError(null);
    try {
      const selection = await open({
        directory: true,
        multiple: false,
        defaultPath: projectRoot,
        title: 'Select a working directory inside this project',
      });
      if (selection === null) {
        return;
      }

      const selectedPath = Array.isArray(selection) ? selection[0] : selection;
      if (!selectedPath?.trim()) {
        setPickerError('The folder picker returned an empty path. Please try again.');
        return;
      }

      const relativePath = await onResolveDirectory(selectedPath);
      if (relativePath !== null) {
        setField('workingDirectory', relativePath);
        touch('workingDirectory');
      }
    } catch (pickerFailure) {
      setPickerError(`The folder picker could not be opened. ${getErrorMessage(pickerFailure)}`);
    }
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (Object.keys(errors).length > 0) {
      return;
    }

    onSubmit({
      name: form.name.trim(),
      workingDirectory: form.workingDirectory.trim(),
      command: form.command.trim(),
      expectedPort: form.expectedPort.trim() ? Number(form.expectedPort.trim()) : null,
      localUrl: form.localUrl.trim() || null,
    });
  }

  const visibleError = pickerError ?? error;

  return (
    <div className="dialog-backdrop">
      <section
        className="dialog-panel service-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="service-dialog-title"
        aria-describedby="service-dialog-description"
      >
        <div className="dialog-heading">
          <p className="dialog-eyebrow">Service configuration</p>
          <h2 id="service-dialog-title">{service ? 'Edit service' : 'Add service'}</h2>
          <p id="service-dialog-description">
            Save this service definition. Saving never starts a process; Start always requires a
            separate confirmation.
          </p>
        </div>

        <form onSubmit={handleSubmit} noValidate>
          <div className="service-form-grid">
            <label className="form-field service-form-full">
              <span>Service name</span>
              <input
                autoFocus
                type="text"
                maxLength={121}
                value={form.name}
                disabled={isBusy}
                aria-invalid={Boolean(fieldError('name'))}
                onBlur={() => touch('name')}
                onChange={(event) => setField('name', event.target.value)}
              />
              {fieldError('name') ? (
                <small className="field-error">{fieldError('name')}</small>
              ) : null}
            </label>

            <div className="form-field service-form-full">
              <span>Working directory</span>
              <div className="directory-field-row">
                <input
                  type="text"
                  value={form.workingDirectory}
                  disabled={isBusy}
                  aria-invalid={Boolean(fieldError('workingDirectory'))}
                  aria-describedby="working-directory-help"
                  onBlur={() => touch('workingDirectory')}
                  onChange={(event) => setField('workingDirectory', event.target.value)}
                />
                <button
                  className="secondary-button browse-button"
                  type="button"
                  disabled={isBusy}
                  onClick={() => void browseForDirectory()}
                >
                  Browse...
                </button>
              </div>
              <small id="working-directory-help" className="field-help">
                Relative to the project root. Use . for the root itself.
              </small>
              {fieldError('workingDirectory') ? (
                <small className="field-error">{fieldError('workingDirectory')}</small>
              ) : null}
            </div>

            <label className="form-field service-form-full">
              <span>Command</span>
              <textarea
                rows={3}
                maxLength={4097}
                value={form.command}
                disabled={isBusy}
                spellCheck={false}
                aria-invalid={Boolean(fieldError('command'))}
                onBlur={() => touch('command')}
                onChange={(event) => setField('command', event.target.value)}
              />
              <small className="field-help">
                Treated as a Windows shell command and run through cmd.exe only after explicit
                confirmation.
              </small>
              {fieldError('command') ? (
                <small className="field-error">{fieldError('command')}</small>
              ) : null}
            </label>

            <label className="form-field">
              <span>Expected port (optional)</span>
              <input
                type="number"
                min="1"
                max="65535"
                step="1"
                inputMode="numeric"
                value={form.expectedPort}
                disabled={isBusy}
                aria-invalid={Boolean(fieldError('expectedPort'))}
                onBlur={() => touch('expectedPort')}
                onChange={(event) => setField('expectedPort', event.target.value)}
              />
              {fieldError('expectedPort') ? (
                <small className="field-error">{fieldError('expectedPort')}</small>
              ) : null}
            </label>

            <label className="form-field">
              <span>Local URL (optional)</span>
              <input
                type="url"
                placeholder="http://localhost:3000"
                maxLength={2049}
                value={form.localUrl}
                disabled={isBusy}
                aria-invalid={Boolean(fieldError('localUrl'))}
                onBlur={() => touch('localUrl')}
                onChange={(event) => setField('localUrl', event.target.value)}
              />
              {fieldError('localUrl') ? (
                <small className="field-error">{fieldError('localUrl')}</small>
              ) : null}
            </label>
          </div>

          <div className="selected-path service-root-path">
            <span>Project root</span>
            <strong title={projectRoot}>{projectRoot}</strong>
          </div>

          {visibleError ? (
            <p className="dialog-error" role="alert">
              {visibleError}
            </p>
          ) : null}

          <div className="dialog-actions">
            <button className="secondary-button" type="button" disabled={isBusy} onClick={onCancel}>
              Cancel
            </button>
            <button className="primary-button" type="submit" disabled={isBusy}>
              {isBusy ? 'Saving...' : 'Save service'}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
