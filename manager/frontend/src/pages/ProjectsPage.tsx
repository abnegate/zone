import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../api/client';
import { useAuth } from '../context/AuthContext';
import type {
  CreateProjectRequest,
  Project,
  ProjectStatus,
  Source,
  UpdateProjectRequest,
} from '../types';
import { getErrors, CreateProjectRequestSchema, UpdateProjectRequestSchema } from '../validation';
import './ProjectsPage.css';

function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
}

const statusLabels: Record<ProjectStatus, string> = {
  active: 'Active',
  on_hold: 'On Hold',
  cancelled: 'Cancelled',
};

const statusColors: Record<ProjectStatus, string> = {
  active: 'status-active',
  on_hold: 'status-on-hold',
  cancelled: 'status-cancelled',
};

export default function ProjectsPage() {
  const { isAuthenticated } = useAuth();

  const [projects, setProjects] = useState<Project[]>([]);
  const [sources, setSources] = useState<Source[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState<ProjectStatus | 'all'>('all');
  const [selectedProject, setSelectedProject] = useState<Project | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showEditModal, setShowEditModal] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [showSourceModal, setShowSourceModal] = useState(false);

  // Form state
  const [formName, setFormName] = useState('');
  const [formDescription, setFormDescription] = useState('');
  const [formStatus, setFormStatus] = useState<ProjectStatus>('active');
  const [formSourceId, setFormSourceId] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  const loadProjects = useCallback(async () => {
    if (!isAuthenticated) return;
    setLoading(true);
    setError(null);
    try {
      const [projectList, sourceList] = await Promise.all([
        client.getProjects(statusFilter === 'all' ? undefined : statusFilter),
        client.getSources(),
      ]);
      setProjects(projectList);
      setSources(sourceList);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load projects');
    } finally {
      setLoading(false);
    }
  }, [isAuthenticated, statusFilter]);

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  const handleCreateProject = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated) return;

    const request: CreateProjectRequest = {
      name: formName.trim(),
      description: formDescription.trim() || undefined,
      status: formStatus,
      source_id: formSourceId || undefined,
    };

    const errors = getErrors(CreateProjectRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setSubmitting(true);
    try {
      const project = await client.createProject(request);
      setProjects((prev) => [project, ...prev]);
      setShowCreateModal(false);
      resetForm();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create project');
    } finally {
      setSubmitting(false);
    }
  };

  const handleUpdateProject = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !selectedProject) return;

    const request: UpdateProjectRequest = {
      name: formName.trim() || undefined,
      description: formDescription.trim() || undefined,
      status: formStatus,
    };

    const errors = getErrors(UpdateProjectRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setSubmitting(true);
    try {
      const updated = await client.updateProject(selectedProject.id, request);
      setProjects((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
      setSelectedProject(updated);
      setShowEditModal(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to update project');
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteProject = async () => {
    if (!isAuthenticated || !selectedProject) return;

    setSubmitting(true);
    try {
      await client.deleteProject(selectedProject.id);
      setProjects((prev) => prev.filter((p) => p.id !== selectedProject.id));
      setSelectedProject(null);
      setShowDeleteConfirm(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete project');
    } finally {
      setSubmitting(false);
    }
  };

  const handleLinkSource = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !selectedProject || !formSourceId) return;

    setSubmitting(true);
    try {
      const updated = await client.linkSource(selectedProject.id, formSourceId);
      setProjects((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
      setSelectedProject(updated);
      setShowSourceModal(false);
      setFormSourceId('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to link source');
    } finally {
      setSubmitting(false);
    }
  };

  const handleUnlinkSource = async () => {
    if (!isAuthenticated || !selectedProject) return;

    try {
      const updated = await client.unlinkSource(selectedProject.id);
      setProjects((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
      setSelectedProject(updated);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to unlink source');
    }
  };

  const openEditModal = (project: Project) => {
    setFormName(project.name);
    setFormDescription(project.description || '');
    setFormStatus(project.status);
    setShowEditModal(true);
  };

  const resetForm = () => {
    setFormName('');
    setFormDescription('');
    setFormStatus('active');
    setFormSourceId('');
    setFieldErrors({});
  };

  // Helper to get source info for display
  const getProjectSource = (project: Project) => {
    return sources.find((s) => s.id === project.source_id);
  };

  return (
    <div className="page page--full projects-page">
      <header className="page-header">
        <div className="header-content">
          <h1>Projects</h1>
          <p className="subtitle">Organize work with GitHub integration</p>
        </div>
        <button
          className="btn btn-primary"
          onClick={() => {
            resetForm();
            setShowCreateModal(true);
          }}
          type="button"
        >
          + New Project
        </button>
      </header>

      <div className="projects-filter">
        <button
          className={`filter-btn ${statusFilter === 'all' ? 'active' : ''}`}
          onClick={() => setStatusFilter('all')}
          type="button"
        >
          All
        </button>
        <button
          className={`filter-btn ${statusFilter === 'active' ? 'active' : ''}`}
          onClick={() => setStatusFilter('active')}
          type="button"
        >
          Active
        </button>
        <button
          className={`filter-btn ${statusFilter === 'on_hold' ? 'active' : ''}`}
          onClick={() => setStatusFilter('on_hold')}
          type="button"
        >
          On Hold
        </button>
        <button
          className={`filter-btn ${statusFilter === 'cancelled' ? 'active' : ''}`}
          onClick={() => setStatusFilter('cancelled')}
          type="button"
        >
          Cancelled
        </button>
      </div>

      {loading ? (
        <div className="projects-loading">
          <span className="spinner" /> Loading projects...
        </div>
      ) : error ? (
        <div className="projects-error">{error}</div>
      ) : projects.length === 0 ? (
        <div className="projects-empty">
          <div className="empty-icon">
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              width="48"
              height="48"
            >
              <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
            </svg>
          </div>
          <h3>No projects yet</h3>
          <p>Create your first project to get started</p>
          <button
            className="btn btn-primary"
            onClick={() => {
              resetForm();
              setShowCreateModal(true);
            }}
            type="button"
          >
            Create Project
          </button>
        </div>
      ) : (
        <div className="projects-grid">
          {projects.map((project) => (
            <div
              key={project.id}
              className={`project-card ${selectedProject?.id === project.id ? 'selected' : ''}`}
              onClick={() => setSelectedProject(project)}
              onKeyDown={(e) => e.key === 'Enter' && setSelectedProject(project)}
              role="button"
              tabIndex={0}
            >
              <div className="project-card-header">
                <h3 className="project-name">{project.name}</h3>
                <span className={`status-badge ${statusColors[project.status]}`}>
                  {statusLabels[project.status]}
                </span>
              </div>
              {project.description && <p className="project-description">{project.description}</p>}
              <div className="project-card-footer">
                {(() => {
                  const source = getProjectSource(project);
                  return source ? (
                    <a
                      href={source.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="source-link"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <span className={`source-type-icon ${source.source_type}`} />
                      {source.name}
                    </a>
                  ) : (
                    <span className="no-source">No source</span>
                  );
                })()}
                <span className="project-date">{formatDate(project.updated_at)}</span>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Project Details Sidebar */}
      {selectedProject && (
        <div className="project-details">
          <div className="details-header">
            <h2>{selectedProject.name}</h2>
            <button
              className="btn btn-icon"
              onClick={() => setSelectedProject(null)}
              type="button"
              aria-label="Close"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                width="20"
                height="20"
              >
                <path d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>

          <div className="details-content">
            <div className="detail-row">
              <span className="detail-label">Status</span>
              <span className={`status-badge ${statusColors[selectedProject.status]}`}>
                {statusLabels[selectedProject.status]}
              </span>
            </div>

            {selectedProject.description && (
              <div className="detail-row">
                <span className="detail-label">Description</span>
                <p className="detail-value">{selectedProject.description}</p>
              </div>
            )}

            <div className="detail-row">
              <span className="detail-label">Source</span>
              {(() => {
                const source = getProjectSource(selectedProject);
                return source ? (
                  <div className="source-detail">
                    <div className="source-info">
                      <span className={`source-type-badge ${source.source_type}`}>
                        {source.source_type}
                      </span>
                      <span className="source-name">{source.name}</span>
                    </div>
                    <a
                      href={source.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="source-url"
                    >
                      {source.url}
                    </a>
                    <button
                      className="btn btn-secondary btn-sm"
                      onClick={handleUnlinkSource}
                      type="button"
                    >
                      Unlink
                    </button>
                  </div>
                ) : (
                  <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => {
                      setFormSourceId('');
                      setShowSourceModal(true);
                    }}
                    type="button"
                  >
                    Link Source
                  </button>
                );
              })()}
            </div>

            <div className="detail-row">
              <span className="detail-label">Created</span>
              <span className="detail-value">{formatDate(selectedProject.created_at)}</span>
            </div>

            <div className="detail-row">
              <span className="detail-label">Updated</span>
              <span className="detail-value">{formatDate(selectedProject.updated_at)}</span>
            </div>
          </div>

          <div className="details-actions">
            <button
              className="btn btn-secondary"
              onClick={() => openEditModal(selectedProject)}
              type="button"
            >
              Edit Project
            </button>
            <button
              className="btn btn-danger"
              onClick={() => setShowDeleteConfirm(true)}
              type="button"
            >
              Delete
            </button>
          </div>
        </div>
      )}

      {/* Create Project Modal */}
      {showCreateModal && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setShowCreateModal(false)}
            onKeyDown={(e) => e.key === 'Escape' && setShowCreateModal(false)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>New Project</h3>
            <form onSubmit={handleCreateProject}>
              <div className="form-group">
                <label htmlFor="project-name">Name</label>
                <input
                  id="project-name"
                  type="text"
                  value={formName}
                  onChange={(e) => setFormName(e.target.value)}
                  placeholder="Project name"
                  className={fieldErrors.name ? 'input-error' : ''}
                />
                {fieldErrors.name && <span className="field-error">{fieldErrors.name}</span>}
              </div>
              <div className="form-group">
                <label htmlFor="project-description">Description</label>
                <textarea
                  id="project-description"
                  value={formDescription}
                  onChange={(e) => setFormDescription(e.target.value)}
                  placeholder="Optional description"
                  rows={3}
                />
              </div>
              <div className="form-group">
                <label htmlFor="project-status">Status</label>
                <select
                  id="project-status"
                  value={formStatus}
                  onChange={(e) => setFormStatus(e.target.value as ProjectStatus)}
                >
                  <option value="active">Active</option>
                  <option value="on_hold">On Hold</option>
                  <option value="cancelled">Cancelled</option>
                </select>
              </div>
              <div className="form-group">
                <label htmlFor="project-source">Source (optional)</label>
                <select
                  id="project-source"
                  value={formSourceId}
                  onChange={(e) => setFormSourceId(e.target.value)}
                >
                  <option value="">No source</option>
                  {sources
                    .filter((s) => s.is_active)
                    .map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name} ({s.source_type})
                      </option>
                    ))}
                </select>
              </div>
              <div className="modal-actions">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowCreateModal(false)}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={submitting || !formName.trim()}
                >
                  {submitting ? (
                    <>
                      <span className="spinner" /> Creating...
                    </>
                  ) : (
                    'Create Project'
                  )}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Edit Project Modal */}
      {showEditModal && selectedProject && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setShowEditModal(false)}
            onKeyDown={(e) => e.key === 'Escape' && setShowEditModal(false)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>Edit Project</h3>
            <form onSubmit={handleUpdateProject}>
              <div className="form-group">
                <label htmlFor="edit-name">Name</label>
                <input
                  id="edit-name"
                  type="text"
                  value={formName}
                  onChange={(e) => setFormName(e.target.value)}
                  placeholder="Project name"
                  className={fieldErrors.name ? 'input-error' : ''}
                />
                {fieldErrors.name && <span className="field-error">{fieldErrors.name}</span>}
              </div>
              <div className="form-group">
                <label htmlFor="edit-description">Description</label>
                <textarea
                  id="edit-description"
                  value={formDescription}
                  onChange={(e) => setFormDescription(e.target.value)}
                  placeholder="Optional description"
                  rows={3}
                />
              </div>
              <div className="form-group">
                <label htmlFor="edit-status">Status</label>
                <select
                  id="edit-status"
                  value={formStatus}
                  onChange={(e) => setFormStatus(e.target.value as ProjectStatus)}
                >
                  <option value="active">Active</option>
                  <option value="on_hold">On Hold</option>
                  <option value="cancelled">Cancelled</option>
                </select>
              </div>
              <div className="modal-actions">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowEditModal(false)}
                >
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={submitting}>
                  {submitting ? (
                    <>
                      <span className="spinner" /> Saving...
                    </>
                  ) : (
                    'Save Changes'
                  )}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal */}
      {showDeleteConfirm && selectedProject && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setShowDeleteConfirm(false)}
            onKeyDown={(e) => e.key === 'Escape' && setShowDeleteConfirm(false)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>Delete Project</h3>
            <p>
              Are you sure you want to delete <strong>{selectedProject.name}</strong>? This action
              cannot be undone.
            </p>
            <div className="modal-actions">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setShowDeleteConfirm(false)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={handleDeleteProject}
                disabled={submitting}
              >
                {submitting ? (
                  <>
                    <span className="spinner" /> Deleting...
                  </>
                ) : (
                  'Delete Project'
                )}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Link Source Modal */}
      {showSourceModal && selectedProject && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setShowSourceModal(false)}
            onKeyDown={(e) => e.key === 'Escape' && setShowSourceModal(false)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>Link Source</h3>
            <form onSubmit={handleLinkSource}>
              <div className="form-group">
                <label htmlFor="source-select">Source</label>
                <select
                  id="source-select"
                  value={formSourceId}
                  onChange={(e) => setFormSourceId(e.target.value)}
                  required
                >
                  <option value="">Select a source...</option>
                  {sources
                    .filter((s) => s.is_active)
                    .map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name} ({s.source_type})
                      </option>
                    ))}
                </select>
                {sources.length === 0 && (
                  <span className="form-hint">
                    No sources configured. Add one in the Sources page.
                  </span>
                )}
              </div>
              <div className="modal-actions">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowSourceModal(false)}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={submitting || !formSourceId}
                >
                  {submitting ? (
                    <>
                      <span className="spinner" /> Linking...
                    </>
                  ) : (
                    'Link Source'
                  )}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
