import { open } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';
import { getErrorMessage } from './api';
import { ProjectCard } from './ProjectCard';
import { ProjectNameDialog } from './ProjectNameDialog';
import { RemoveProjectDialog } from './RemoveProjectDialog';
import type { Project, ProjectsController } from './types';

interface ProjectsViewProps {
  controller: ProjectsController;
  onSelectProject: (project: Project) => void;
}

type ProjectEditor =
  { mode: 'add'; rootPath: string; initialName: string } | { mode: 'rename'; project: Project };

function AddIcon() {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="M10 4v12M4 10h12" />
    </svg>
  );
}

function EmptyProjectsIcon() {
  return (
    <svg viewBox="0 0 48 48" aria-hidden="true">
      <path d="M7.5 15.5v20A5.5 5.5 0 0 0 13 41h22a5.5 5.5 0 0 0 5.5-5.5v-16A5.5 5.5 0 0 0 35 14H23l-4-5H13a5.5 5.5 0 0 0-5.5 5.5v1Z" />
      <path d="M18 28h12M24 22v12" />
    </svg>
  );
}

function selectedFolderName(path: string): string {
  const withoutTrailingSeparators = path.replace(/[\\/]+$/, '');
  const segments = withoutTrailingSeparators.split(/[\\/]/);
  return segments[segments.length - 1] || withoutTrailingSeparators || path;
}

export function ProjectsView({ controller, onSelectProject }: ProjectsViewProps) {
  const [editor, setEditor] = useState<ProjectEditor | null>(null);
  const [projectToRemove, setProjectToRemove] = useState<Project | null>(null);
  const [pickerError, setPickerError] = useState<string | null>(null);

  async function selectProjectFolder() {
    controller.clearError();
    setPickerError(null);

    try {
      const selection = await open({
        directory: true,
        multiple: false,
        title: 'Select a project folder',
      });

      if (selection === null) {
        return;
      }

      const rootPath = Array.isArray(selection) ? selection[0] : selection;
      if (!rootPath?.trim()) {
        setPickerError('The folder picker returned an empty path. Please try again.');
        return;
      }

      setEditor({
        mode: 'add',
        rootPath,
        initialName: selectedFolderName(rootPath),
      });
    } catch (error) {
      setPickerError(`The folder picker could not be opened. ${getErrorMessage(error)}`);
    }
  }

  async function saveProjectName(name: string) {
    if (!editor) {
      return;
    }

    const saved =
      editor.mode === 'add'
        ? await controller.addProject(editor.rootPath, name)
        : await controller.renameProject(editor.project.id, name);

    if (saved) {
      setEditor(null);
    }
  }

  async function removeProject() {
    if (!projectToRemove) {
      return;
    }

    if (await controller.removeProject(projectToRemove.id)) {
      setProjectToRemove(null);
    }
  }

  function beginRename(project: Project) {
    controller.clearError();
    setPickerError(null);
    setEditor({ mode: 'rename', project });
  }

  function beginRemove(project: Project) {
    controller.clearError();
    setPickerError(null);
    setProjectToRemove(project);
  }

  const visibleError = pickerError ?? (!editor && !projectToRemove ? controller.error : null);

  return (
    <section className="projects-view" aria-labelledby="page-title">
      <header className="page-header">
        <div>
          <p className="eyebrow">Project workspace</p>
          <h1 id="page-title">Server Dashboard</h1>
          <p className="page-description">
            Organize local projects and start configured services only when you explicitly confirm
            their commands.
          </p>
        </div>

        <button
          className="primary-button"
          type="button"
          disabled={controller.isLoading || controller.isMutating}
          onClick={() => void selectProjectFolder()}
        >
          <AddIcon />
          Add project
        </button>
      </header>

      {visibleError ? (
        <div className="error-banner" role="alert">
          <div>
            <strong>Server Dashboard could not complete the request.</strong>
            <p>{visibleError}</p>
          </div>
          <button
            type="button"
            aria-label="Dismiss error"
            onClick={() => {
              setPickerError(null);
              controller.clearError();
            }}
          >
            Dismiss
          </button>
        </div>
      ) : null}

      {controller.isLoading ? (
        <div className="loading-state" role="status">
          <span className="loading-spinner" aria-hidden="true" />
          Loading projects...
        </div>
      ) : controller.projects.length === 0 ? (
        <div className="empty-state">
          <div className="empty-status">
            <span className="empty-status-dot" aria-hidden="true" />0 projects configured
          </div>
          <div className="empty-icon">
            <EmptyProjectsIcon />
          </div>
          <h2>No projects yet</h2>
          <p>
            Add a project folder to register it locally. Nothing runs automatically, and Server
            Dashboard never deletes project files.
          </p>
        </div>
      ) : (
        <div className="projects-grid" aria-label="Registered projects">
          {controller.projects.map((project) => (
            <ProjectCard
              key={project.id}
              project={project}
              disabled={controller.isMutating}
              active={controller.isProjectActive(project.id)}
              onManage={onSelectProject}
              onOpen={(selectedProject) => void controller.openProjectFolder(selectedProject.id)}
              onRename={beginRename}
              onRemove={beginRemove}
            />
          ))}
        </div>
      )}

      {editor ? (
        <ProjectNameDialog
          title={editor.mode === 'add' ? 'Add this project' : 'Rename project'}
          description={
            editor.mode === 'add'
              ? 'Confirm the name shown in Server Dashboard. The selected folder will not be scanned.'
              : 'Change only the name shown in Server Dashboard. The folder name on disk stays unchanged.'
          }
          rootPath={editor.mode === 'add' ? editor.rootPath : editor.project.rootPath}
          initialName={editor.mode === 'add' ? editor.initialName : editor.project.name}
          submitLabel={editor.mode === 'add' ? 'Save project' : 'Save name'}
          error={controller.error}
          isBusy={controller.isMutating}
          onCancel={() => {
            controller.clearError();
            setEditor(null);
          }}
          onSubmit={(name) => void saveProjectName(name)}
        />
      ) : null}

      {projectToRemove ? (
        <RemoveProjectDialog
          project={projectToRemove}
          error={controller.error}
          isBusy={controller.isMutating}
          onCancel={() => {
            controller.clearError();
            setProjectToRemove(null);
          }}
          onConfirm={() => void removeProject()}
        />
      ) : null}
    </section>
  );
}
