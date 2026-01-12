import { type FormEvent, useCallback, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { client } from '../../../api/client';
import { useAuth } from '../../../features/auth';
import { useProjects, useSyncConfigs } from '../hooks';
import { CreateProjectWizard } from '../components';
import type {
  CreateSyncConfigRequest,
  Project,
  ProjectStatus,
  SyncDirection,
  SyncProvider,
  UpdateProjectRequest,
} from '../types';
import {
  CreateSyncConfigRequestSchema,
  UpdateProjectRequestSchema,
} from '../schemas';
import { getErrors } from '../../../validation';
import { formatDate } from '../utils/formatters';
import './ProjectsPage.css';

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

  // Use projects hook with status filter
  const [statusFilter, setStatusFilter] = useState<ProjectStatus | 'all'>('all');
  const {
    projects,
    loading,
    error,
    createProject: createProjectMutation,
    updateProject: updateProjectMutation,
    deleteProject: deleteProjectMutation,
  } = useProjects(statusFilter);

  // Sources query
  const { data: sources = [] } = useQuery({
    queryKey: ['sources'],
    queryFn: () => client.getSources(),
    enabled: isAuthenticated
  });

  // State
  const [selectedProject, setSelectedProject] = useState<Project | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showEditModal, setShowEditModal] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [showSourceModal, setShowSourceModal] = useState(false);
  const [showSyncModal, setShowSyncModal] = useState(false);

  // Use sync configs hook for selected project
  const {
    configs: syncConfigs,
    loading: syncLoading,
    createSyncConfig: createSyncConfigMutation,
    deleteSyncConfig: deleteSyncConfigMutation,
  } = useSyncConfigs(selectedProject?.id || null);

  // Form state
  const [formName, setFormName] = useState('');
  const [formDescription, setFormDescription] = useState('');
  const [formStatus, setFormStatus] = useState<ProjectStatus>('active');
  const [formSourceId, setFormSourceId] = useState('');
  const [formSyncProvider, setFormSyncProvider] = useState<SyncProvider>('github');
  const [formSyncDirection, setFormSyncDirection] = useState<SyncDirection>('bidirectional');
  const [formSyncRepoUrl, setFormSyncRepoUrl] = useState('');
  const [formSyncProjectId, setFormSyncProjectId] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  const handleProjectCreated = useCallback(
    (_project: Project) => {
      // Project is already added to the list by the hook
    },
    []
  );

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
      const updated = await updateProjectMutation(selectedProject.id, request);
      setSelectedProject(updated);
      setShowEditModal(false);
    } catch (err) {
      console.error('Failed to update project:', err);
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteProject = async () => {
    if (!isAuthenticated || !selectedProject) return;

    setSubmitting(true);
    try {
      await deleteProjectMutation(selectedProject.id);
      setSelectedProject(null);
      setShowDeleteConfirm(false);
    } catch (err) {
      console.error('Failed to delete project:', err);
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
      setSelectedProject(updated);
      setShowSourceModal(false);
      setFormSourceId('');
    } catch (err) {
      console.error('Failed to link source:', err);
    } finally {
      setSubmitting(false);
    }
  };

  const handleUnlinkSource = async () => {
    if (!isAuthenticated || !selectedProject) return;

    try {
      const updated = await client.unlinkSource(selectedProject.id);
      setSelectedProject(updated);
    } catch (err) {
      console.error('Failed to unlink source:', err);
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
    setFormSyncProvider('github');
    setFormSyncDirection('bidirectional');
    setFormSyncRepoUrl('');
    setFormSyncProjectId('');
    setFieldErrors({});
  };

  const handleCreateSyncConfig = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !selectedProject) return;

    const request: CreateSyncConfigRequest = {
      provider: formSyncProvider,
      direction: formSyncDirection,
      external_repo_url: formSyncProvider === 'github' ? formSyncRepoUrl : undefined,
      external_project_id: formSyncProvider === 'linear' ? formSyncProjectId : undefined,
    };

    const errors = getErrors(CreateSyncConfigRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setSubmitting(true);
    try {
      await createSyncConfigMutation(request);
      setShowSyncModal(false);
      resetForm();
    } catch (err) {
      console.error('Failed to create sync config:', err);
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteSyncConfig = async (configId: string) => {
    if (!isAuthenticated || !selectedProject) return;

    try {
      await deleteSyncConfigMutation(configId);
    } catch (err) {
      console.error('Failed to delete sync config:', err);
    }
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

            {/* Sync Configuration Section */}
            <div className="sync-config-section">
              <div className="sync-config-header">
                <h3>External Sync</h3>
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={() => {
                    resetForm();
                    setShowSyncModal(true);
                  }}
                  type="button"
                >
                  + Add Sync
                </button>
              </div>

              {syncLoading ? (
                <div className="sync-config-empty">
                  <span className="spinner" /> Loading...
                </div>
              ) : syncConfigs.length === 0 ? (
                <div className="sync-config-empty">
                  No sync configurations. Add one to sync with GitHub or Linear.
                </div>
              ) : (
                <div className="sync-config-list">
                  {syncConfigs.map((config) => (
                    <div key={config.id} className="sync-config-item">
                      <div className="sync-config-info">
                        <span className={`sync-provider-badge ${config.provider}`}>
                          {config.provider}
                        </span>
                        <span className="sync-direction">{config.direction}</span>
                        {config.external_repo_url && (
                          <a
                            href={config.external_repo_url}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="sync-external-link"
                            onClick={(e) => e.stopPropagation()}
                          >
                            {config.external_repo_url}
                          </a>
                        )}
                        {config.external_project_id && (
                          <span className="sync-external-link">{config.external_project_id}</span>
                        )}
                      </div>
                      <button
                        className="btn btn-danger btn-sm"
                        onClick={() => handleDeleteSyncConfig(config.id)}
                        type="button"
                      >
                        Remove
                      </button>
                    </div>
                  ))}
                </div>
              )}
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

      {/* Create Project Wizard */}
      <CreateProjectWizard
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        onCreated={handleProjectCreated}
        createProject={createProjectMutation}
      />

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

      {/* Add Sync Config Modal */}
      {showSyncModal && selectedProject && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setShowSyncModal(false)}
            onKeyDown={(e) => e.key === 'Escape' && setShowSyncModal(false)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>Add External Sync</h3>
            <form onSubmit={handleCreateSyncConfig}>
              <div className="form-group">
                <label htmlFor="sync-provider">Provider</label>
                <select
                  id="sync-provider"
                  value={formSyncProvider}
                  onChange={(e) => setFormSyncProvider(e.target.value as SyncProvider)}
                >
                  <option value="github">GitHub</option>
                  <option value="linear">Linear</option>
                </select>
              </div>
              <div className="form-group">
                <label htmlFor="sync-direction">Direction</label>
                <select
                  id="sync-direction"
                  value={formSyncDirection}
                  onChange={(e) => setFormSyncDirection(e.target.value as SyncDirection)}
                >
                  <option value="inbound">Inbound (External to Zone)</option>
                  <option value="outbound">Outbound (Zone to External)</option>
                  <option value="bidirectional">Bidirectional</option>
                </select>
              </div>
              {formSyncProvider === 'github' && (
                <div className="form-group">
                  <label htmlFor="sync-repo-url">Repository URL</label>
                  <input
                    id="sync-repo-url"
                    type="url"
                    value={formSyncRepoUrl}
                    onChange={(e) => setFormSyncRepoUrl(e.target.value)}
                    placeholder="https://github.com/owner/repo"
                    className={fieldErrors.external_repo_url ? 'input-error' : ''}
                  />
                  {fieldErrors.external_repo_url && (
                    <span className="field-error">{fieldErrors.external_repo_url}</span>
                  )}
                </div>
              )}
              {formSyncProvider === 'linear' && (
                <div className="form-group">
                  <label htmlFor="sync-project-id">Project ID</label>
                  <input
                    id="sync-project-id"
                    type="text"
                    value={formSyncProjectId}
                    onChange={(e) => setFormSyncProjectId(e.target.value)}
                    placeholder="LINEAR-123"
                    className={fieldErrors.external_project_id ? 'input-error' : ''}
                  />
                  {fieldErrors.external_project_id && (
                    <span className="field-error">{fieldErrors.external_project_id}</span>
                  )}
                </div>
              )}
              <div className="modal-actions">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowSyncModal(false)}
                >
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={submitting}>
                  {submitting ? (
                    <>
                      <span className="spinner" /> Adding...
                    </>
                  ) : (
                    'Add Sync Config'
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
