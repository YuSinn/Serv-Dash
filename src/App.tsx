import { useState } from 'react';
import './App.css';
import { ProjectDetailView } from './features/projects/ProjectDetailView';
import { ProjectsView } from './features/projects/ProjectsView';
import { useProjects } from './features/projects/useProjects';

function DashboardMark() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M5.75 4.5h12.5A1.75 1.75 0 0 1 20 6.25v3.5a1.75 1.75 0 0 1-1.75 1.75H5.75A1.75 1.75 0 0 1 4 9.75v-3.5A1.75 1.75 0 0 1 5.75 4.5Z" />
      <path d="M5.75 12.5h12.5A1.75 1.75 0 0 1 20 14.25v3.5a1.75 1.75 0 0 1-1.75 1.75H5.75A1.75 1.75 0 0 1 4 17.75v-3.5a1.75 1.75 0 0 1 1.75-1.75Z" />
      <path d="M7.5 8h.01M7.5 16h.01M10.5 8h6M10.5 16h6" />
    </svg>
  );
}

function ProjectsIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3.75 7.75v9.5A2.75 2.75 0 0 0 6.5 20h11a2.75 2.75 0 0 0 2.75-2.75v-7.5A2.75 2.75 0 0 0 17.5 7h-5.67l-1.66-2H6.5a2.75 2.75 0 0 0-2.75 2.75Z" />
    </svg>
  );
}

function App() {
  const projectsController = useProjects();
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const projectCount = projectsController.projects.length;
  const selectedProject = projectsController.projects.find(
    (project) => project.id === selectedProjectId,
  );

  function showProjects() {
    projectsController.clearError();
    setSelectedProjectId(null);
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <DashboardMark />
          </span>
          <span className="brand-copy">
            <strong>Server</strong>
            <span>Dashboard</span>
          </span>
        </div>

        <nav className="sidebar-nav" aria-label="Primary navigation">
          <p className="nav-label">Workspace</p>
          <button
            className="nav-item nav-item-active nav-item-button"
            type="button"
            aria-current="page"
            onClick={showProjects}
          >
            <ProjectsIcon />
            <span>Projects</span>
            <span className="nav-count">{projectCount}</span>
          </button>
        </nav>

        <div className="sidebar-status" role="status">
          <span className="status-dot" aria-hidden="true" />
          <span>
            <strong>Local workspace</strong>
            <small>
              {projectsController.isLoading
                ? 'Loading saved projects'
                : selectedProject
                  ? 'Managing local service processes'
                  : projectCount === 0
                    ? 'Ready for your first project'
                    : `${projectCount} ${projectCount === 1 ? 'project' : 'projects'} saved locally`}
            </small>
          </span>
        </div>
      </aside>

      <main className="workspace" id="projects">
        {selectedProject ? (
          <ProjectDetailView
            project={selectedProject}
            projectError={projectsController.error}
            isProjectMutating={projectsController.isMutating}
            onBack={showProjects}
            onClearProjectError={projectsController.clearError}
            onOpenRoot={() => void projectsController.openProjectFolder(selectedProject.id)}
          />
        ) : (
          <ProjectsView
            controller={projectsController}
            onSelectProject={(project) => {
              projectsController.clearError();
              setSelectedProjectId(project.id);
            }}
          />
        )}
      </main>
    </div>
  );
}

export default App;
