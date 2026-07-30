import type { Project } from './types';

interface ProjectCardProps {
  project: Project;
  disabled: boolean;
  active: boolean;
  onManage: (project: Project) => void;
  onOpen: (project: Project) => void;
  onRename: (project: Project) => void;
  onRemove: (project: Project) => void;
}

function FolderIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3.75 7.75v9.5A2.75 2.75 0 0 0 6.5 20h11a2.75 2.75 0 0 0 2.75-2.75v-7.5A2.75 2.75 0 0 0 17.5 7h-5.67l-1.66-2H6.5a2.75 2.75 0 0 0-2.75 2.75Z" />
    </svg>
  );
}

export function ProjectCard({
  project,
  disabled,
  active,
  onManage,
  onOpen,
  onRename,
  onRemove,
}: ProjectCardProps) {
  return (
    <article className="project-card">
      <div className="project-card-heading">
        <span className="project-folder-icon">
          <FolderIcon />
        </span>
        <div>
          <h2>{project.name}</h2>
          <span className={`project-status${active ? ' project-status-active' : ''}`}>
            <span aria-hidden="true" />
            {active ? 'Process active' : 'Registered'}
          </span>
        </div>
      </div>

      <p className="project-path" title={project.rootPath}>
        {project.rootPath}
      </p>

      <button
        className="secondary-button open-folder-button"
        type="button"
        disabled={disabled}
        onClick={() => onManage(project)}
      >
        Manage services
      </button>

      {active ? (
        <p className="project-runtime-warning">
          Stop active services before renaming or removing this project.
        </p>
      ) : null}

      <div className="project-card-actions">
        <button
          className="text-button"
          type="button"
          disabled={disabled}
          onClick={() => onOpen(project)}
        >
          Open folder
        </button>
        <button
          className="text-button"
          type="button"
          disabled={disabled || active}
          onClick={() => onRename(project)}
        >
          Rename
        </button>
        <button
          className="text-button text-button-danger"
          type="button"
          disabled={disabled || active}
          onClick={() => onRemove(project)}
        >
          Remove
        </button>
      </div>
    </article>
  );
}
