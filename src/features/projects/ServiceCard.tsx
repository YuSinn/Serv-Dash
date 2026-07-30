import { canStopRuntime } from './runtimeReconciliation';
import type { ServiceDefinition, ServiceRuntime } from './types';
import { isRuntimeActive } from './types';

interface ServiceCardProps {
  service: ServiceDefinition;
  runtime: ServiceRuntime;
  disabled: boolean;
  onStart: (service: ServiceDefinition) => void;
  onStop: (service: ServiceDefinition) => void;
  onViewLogs: (service: ServiceDefinition) => void;
  onEdit: (service: ServiceDefinition) => void;
  onRemove: (service: ServiceDefinition) => void;
}

const STATUS_LABELS: Record<ServiceRuntime['status'], string> = {
  stopped: 'Stopped',
  starting: 'Starting',
  running: 'Process running',
  stopping: 'Stopping',
  exited: 'Exited',
  failed: 'Failed',
};

function ServiceIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="4" y="5" width="16" height="6" rx="2" />
      <rect x="4" y="13" width="16" height="6" rx="2" />
      <path d="M8 8h.01M8 16h.01M12 8h5M12 16h5" />
    </svg>
  );
}

function formatStartedAt(timestamp: string): string {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? timestamp : date.toLocaleString();
}

export function ServiceCard({
  service,
  runtime,
  disabled,
  onStart,
  onStop,
  onViewLogs,
  onEdit,
  onRemove,
}: ServiceCardProps) {
  const active = isRuntimeActive(runtime.status);
  const canStop = canStopRuntime(runtime);

  return (
    <article className="service-card">
      <div className="service-card-header">
        <span className="service-icon">
          <ServiceIcon />
        </span>
        <div>
          <h2>{service.name}</h2>
          <span className="configuration-badge">Configured local command</span>
        </div>
      </div>

      <div className={`runtime-summary runtime-${runtime.status}`}>
        <span className="runtime-status-dot" aria-hidden="true" />
        <strong>{STATUS_LABELS[runtime.status]}</strong>
        {active && runtime.pid !== null ? <span>PID {runtime.pid}</span> : null}
      </div>

      {runtime.status === 'running' ? (
        <p className="runtime-health-note">
          The process is running. Health is not checked in this phase.
        </p>
      ) : null}
      {active && runtime.startedAt ? (
        <p className="runtime-started-at">Started {formatStartedAt(runtime.startedAt)}</p>
      ) : null}
      {!active && runtime.exitCode !== null ? (
        <p className="runtime-exit-code">Exit code: {runtime.exitCode}</p>
      ) : null}
      {runtime.error ? (
        <p className="runtime-card-error" role="status">
          {runtime.error}
        </p>
      ) : null}

      <dl className="service-details">
        <div>
          <dt>Working directory</dt>
          <dd title={service.workingDirectory}>{service.workingDirectory}</dd>
        </div>
        <div>
          <dt>Configured command</dt>
          <dd title={service.command}>{service.command}</dd>
        </div>
        {service.expectedPort !== null ? (
          <div>
            <dt>Expected port</dt>
            <dd>{service.expectedPort}</dd>
          </div>
        ) : null}
        {service.localUrl ? (
          <div>
            <dt>Local URL</dt>
            <dd title={service.localUrl}>{service.localUrl}</dd>
          </div>
        ) : null}
      </dl>

      {active ? (
        <p className="active-service-warning">
          Stop this service before editing or removing its configuration.
        </p>
      ) : null}

      <div className="service-run-actions">
        <button
          className="primary-button service-action-button"
          type="button"
          disabled={disabled || active}
          onClick={() => onStart(service)}
        >
          Start
        </button>
        <button
          className="danger-button service-action-button"
          type="button"
          disabled={disabled || !canStop}
          title="Force-terminates this service's Windows Job and all descendants"
          onClick={() => onStop(service)}
        >
          Force stop
        </button>
        <button
          className="secondary-button service-action-button"
          type="button"
          disabled={disabled}
          onClick={() => onViewLogs(service)}
        >
          View logs
        </button>
      </div>

      <div className="service-card-actions">
        <p>Runtime data remains in memory only.</p>
        <button
          className="text-button"
          type="button"
          disabled={disabled || active}
          onClick={() => onEdit(service)}
        >
          Edit
        </button>
        <button
          className="text-button text-button-danger"
          type="button"
          disabled={disabled || active}
          onClick={() => onRemove(service)}
        >
          Remove
        </button>
      </div>
    </article>
  );
}
