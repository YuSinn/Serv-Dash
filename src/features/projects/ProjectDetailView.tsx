import { useState } from 'react';
import './services.css';
import './runtime.css';
import { RemoveServiceDialog } from './RemoveServiceDialog';
import { ServiceCard } from './ServiceCard';
import { ServiceDetectionControl } from './ServiceDetectionControl';
import { ServiceDialog } from './ServiceDialog';
import { ServiceLogsPanel } from './ServiceLogsPanel';
import { StartServiceDialog } from './StartServiceDialog';
import type { Project, ServiceDefinition, ServiceInput, ServiceStartPreview } from './types';
import { useServiceRuntime } from './useServiceRuntime';
import { useServices } from './useServices';

interface ProjectDetailViewProps {
  project: Project;
  projectError: string | null;
  isProjectMutating: boolean;
  onBack: () => void;
  onClearProjectError: () => void;
  onOpenRoot: () => void;
}

type ServiceEditor = { mode: 'add' } | { mode: 'edit'; service: ServiceDefinition };

function AddIcon() {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="M10 4v12M4 10h12" />
    </svg>
  );
}

function BackIcon() {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="m12.5 4.5-5.5 5.5 5.5 5.5M7.5 10H17" />
    </svg>
  );
}

function EmptyServicesIcon() {
  return (
    <svg viewBox="0 0 48 48" aria-hidden="true">
      <rect x="8" y="9" width="32" height="12" rx="4" />
      <rect x="8" y="27" width="32" height="12" rx="4" />
      <path d="M15 15h.01M15 33h.01M22 15h11M22 33h11" />
    </svg>
  );
}

export function ProjectDetailView({
  project,
  projectError,
  isProjectMutating,
  onBack,
  onClearProjectError,
  onOpenRoot,
}: ProjectDetailViewProps) {
  const controller = useServices(project.id);
  const runtime = useServiceRuntime(project.id, controller.services);
  const [editor, setEditor] = useState<ServiceEditor | null>(null);
  const [serviceToRemove, setServiceToRemove] = useState<ServiceDefinition | null>(null);
  const [startPreview, setStartPreview] = useState<ServiceStartPreview | null>(null);

  const isPersistenceBusy = controller.isMutating || isProjectMutating;
  const visibleError =
    !editor && !serviceToRemove && !startPreview
      ? (runtime.error ?? controller.error ?? projectError)
      : null;
  const selectedLogService =
    controller.services.find((service) => service.id === runtime.selectedServiceId) ?? null;
  const selectedLogRuntime = selectedLogService ? runtime.runtimeFor(selectedLogService.id) : null;

  function clearErrors() {
    runtime.clearError();
    controller.clearError();
    onClearProjectError();
  }

  function beginAdd() {
    clearErrors();
    setEditor({ mode: 'add' });
  }

  function beginEdit(service: ServiceDefinition) {
    clearErrors();
    setEditor({ mode: 'edit', service });
  }

  function beginRemove(service: ServiceDefinition) {
    clearErrors();
    setServiceToRemove(service);
  }

  async function beginStart(service: ServiceDefinition) {
    clearErrors();
    runtime.selectLogs(service.id);
    const preview = await runtime.prepareStart(service.id);
    if (preview) {
      setStartPreview(preview);
    }
  }

  async function confirmStart() {
    if (!startPreview) {
      return;
    }
    if (await runtime.start(startPreview.serviceId)) {
      setStartPreview(null);
    }
  }

  async function saveService(input: ServiceInput) {
    if (!editor) {
      return;
    }

    const saved =
      editor.mode === 'add'
        ? await controller.addService(input)
        : await controller.updateService(editor.service.id, input);
    if (saved) {
      setEditor(null);
    }
  }

  async function removeService() {
    if (!serviceToRemove) {
      return;
    }
    if (await controller.removeService(serviceToRemove.id)) {
      if (runtime.selectedServiceId === serviceToRemove.id) {
        runtime.selectLogs(null);
      }
      setServiceToRemove(null);
    }
  }

  return (
    <section className="project-detail-view" aria-labelledby="project-detail-title">
      <button
        className="back-button"
        type="button"
        disabled={isPersistenceBusy}
        onClick={() => {
          clearErrors();
          onBack();
        }}
      >
        <BackIcon />
        Back to projects
      </button>

      <header className="detail-header">
        <div className="detail-heading-copy">
          <div className="detail-label-row">
            <p className="eyebrow">Project services</p>
            <span className="configuration-badge">Explicit local execution</span>
          </div>
          <h1 id="project-detail-title">{project.name}</h1>
          <p className="detail-root-path" title={project.rootPath}>
            {project.rootPath}
          </p>
        </div>

        <div className="detail-header-actions">
          <button
            className="secondary-button"
            type="button"
            disabled={isPersistenceBusy}
            onClick={onOpenRoot}
          >
            Open root folder
          </button>
          <ServiceDetectionControl projectId={project.id} />
          <button
            className="primary-button"
            type="button"
            disabled={controller.isLoading || isPersistenceBusy}
            onClick={beginAdd}
          >
            <AddIcon />
            Add service
          </button>
        </div>
      </header>

      <div className="configuration-notice runtime-notice">
        <strong>Commands run only after confirmation.</strong>
        <span>
          &quot;Process running&quot; means only that Windows reports an active process; it is not a
          health check. Runtime state and logs are never persisted.
        </span>
      </div>

      {visibleError ? (
        <div className="error-banner detail-error-banner" role="alert">
          <div>
            <strong>Server Dashboard could not complete the request.</strong>
            <p>{visibleError}</p>
          </div>
          <button type="button" aria-label="Dismiss error" onClick={clearErrors}>
            Dismiss
          </button>
        </div>
      ) : null}

      {controller.isLoading ? (
        <div className="services-loading" role="status">
          <span className="loading-spinner" aria-hidden="true" />
          Loading service configuration...
        </div>
      ) : controller.services.length === 0 ? (
        <div className="services-empty-state">
          <span className="services-empty-icon">
            <EmptyServicesIcon />
          </span>
          <h2>No services configured</h2>
          <p>
            Add a service definition manually. Saving configuration never starts a process; every
            Start requires a separate confirmation.
          </p>
          <button
            className="secondary-button"
            type="button"
            disabled={isPersistenceBusy}
            onClick={beginAdd}
          >
            Add service
          </button>
        </div>
      ) : (
        <>
          <div className="services-grid" aria-label="Configured services">
            {controller.services.map((service) => (
              <ServiceCard
                key={service.id}
                service={service}
                runtime={runtime.runtimeFor(service.id)}
                disabled={isPersistenceBusy || runtime.busyServiceIds.has(service.id)}
                onStart={(selected) => void beginStart(selected)}
                onStop={(selected) => {
                  runtime.selectLogs(selected.id);
                  void runtime.stop(selected.id);
                }}
                onViewLogs={(selected) => runtime.selectLogs(selected.id)}
                onEdit={beginEdit}
                onRemove={beginRemove}
              />
            ))}
          </div>

          <ServiceLogsPanel
            service={selectedLogService}
            runtime={selectedLogRuntime}
            logs={runtime.selectedLogs}
            isBusy={selectedLogService ? runtime.busyServiceIds.has(selectedLogService.id) : false}
            onClear={() => {
              if (selectedLogService) {
                void runtime.clearLogs(selectedLogService.id);
              }
            }}
          />
        </>
      )}

      {editor ? (
        <ServiceDialog
          key={editor.mode === 'edit' ? editor.service.id : 'new-service'}
          projectRoot={project.rootPath}
          service={editor.mode === 'edit' ? editor.service : null}
          services={controller.services}
          error={controller.error}
          isBusy={controller.isMutating}
          onCancel={() => {
            controller.clearError();
            setEditor(null);
          }}
          onResolveDirectory={controller.resolveWorkingDirectory}
          onSubmit={(input) => void saveService(input)}
        />
      ) : null}

      {serviceToRemove ? (
        <RemoveServiceDialog
          service={serviceToRemove}
          error={controller.error}
          isBusy={controller.isMutating}
          onCancel={() => {
            controller.clearError();
            setServiceToRemove(null);
          }}
          onConfirm={() => void removeService()}
        />
      ) : null}

      {startPreview ? (
        <StartServiceDialog
          preview={startPreview}
          error={runtime.error}
          isBusy={runtime.busyServiceIds.has(startPreview.serviceId)}
          onCancel={() => {
            runtime.clearError();
            setStartPreview(null);
          }}
          onConfirm={() => void confirmStart()}
        />
      ) : null}
    </section>
  );
}
