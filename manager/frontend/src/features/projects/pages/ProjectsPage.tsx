import { useQuery } from '@tanstack/react-query';
import { Badge, Button, EmptyState, Modal, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { client } from '../../../api/client';
import { projectsApi } from '../../../api/projects';
import { useAuth } from '../../../features/auth';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { getErrors } from '../../../validation';
import { AutomationPanel, AutoProjectModal, CreateProjectWizard } from '../components';
import { useAutomation, useProjects, useSyncConfigs } from '../hooks';
import { CreateSyncConfigRequestSchema, UpdateProjectRequestSchema } from '../schemas';
import type {
  CreateSyncConfigRequest,
  Project,
  ProjectStatus,
  SyncConfig,
  SyncDirection,
  SyncProvider,
  UpdateProjectRequest,
} from '../types';
import { formatDate } from '../utils/formatters';
import './ProjectsPage.css';
import { useWorkspace } from '../../../shared/context';

const statusLabels: Record<ProjectStatus, string> = {
  active: 'Active',
  on_hold: 'On Hold',
  cancelled: 'Cancelled',
};

const statusVariants: Record<ProjectStatus, 'success' | 'warning' | 'destructive'> = {
  active: 'success',
  on_hold: 'warning',
  cancelled: 'destructive',
};

function FolderIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
    </svg>
  );
}

export default function ProjectsPage() {
  const { isAuthenticated } = useAuth();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedProjectId = searchParams.get('id');

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
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const { data: sources = [] } = useQuery({
    queryKey: ['sources', workspaceId],
    queryFn: () => client.getSources(workspaceId as string),
    enabled: isAuthenticated && !!workspaceId,
  });

  // State
  const [selectedProject, setSelectedProject] = useState<Project | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showAutoModal, setShowAutoModal] = useState(false);
  const [togglingAuto, setTogglingAuto] = useState(false);
  const [automationActionError, setAutomationActionError] = useState<string | null>(null);
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

  // A link such as /projects?id=… (from a planner receipt) selects that project once
  // loaded, once: the router applies a URL change as a transition, so closing the
  // panel would otherwise be re-selected by this effect before the parameter is gone
  const honouredLink = useRef<string | null>(null);
  useEffect(() => {
    // Once the URL has really moved on, the same link may be followed again
    if (honouredLink.current && honouredLink.current !== requestedProjectId) {
      honouredLink.current = null;
    }
    if (!requestedProjectId || selectedProject) return;
    if (honouredLink.current === requestedProjectId) return;
    const match = projects.find((project) => project.id === requestedProjectId);
    if (match) {
      honouredLink.current = requestedProjectId;
      setSelectedProject(match);
    }
  }, [requestedProjectId, projects, selectedProject]);

  // An automation error belongs to the project it happened on
  const selectProject = (project: Project) => {
    setAutomationActionError(null);
    setSelectedProject(project);
  };

  // Closing the panel also clears the deep link from the URL
  const closeDetails = () => {
    setSelectedProject(null);
    setAutomationActionError(null);
    if (searchParams.has('id')) {
      const next = new URLSearchParams(searchParams);
      next.delete('id');
      setSearchParams(next, { replace: true });
    }
  };

  // Automation state, re-read while the selected project runs itself
  const {
    automation,
    loading: automationLoading,
    error: automationError,
    resume: resumeAutomation,
    resuming,
  } = useAutomation(selectedProject?.auto ? selectedProject.id : null, !!selectedProject?.auto);

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
  const [operationError, setOperationError] = useState<string | null>(null);

  const failed = (err: unknown, fallback: string) =>
    setOperationError(err instanceof Error ? err.message : fallback);
  const modalOpen = showEditModal || showDeleteConfirm || showSourceModal || showSyncModal;

  const openModal = (open: (value: boolean) => void) => {
    setOperationError(null);
    open(true);
  };

  const handleProjectCreated = useCallback((_project: Project) => {
    // Project is already added to the list by the hook
  }, []);

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
    setOperationError(null);
    try {
      const updated = await updateProjectMutation(selectedProject.id, request);
      setSelectedProject(updated);
      setShowEditModal(false);
    } catch (err) {
      failed(err, 'Failed to update project');
    } finally {
      setSubmitting(false);
    }
  };

  const handleToggleAuto = async () => {
    if (!isAuthenticated || !selectedProject) return;

    setAutomationActionError(null);
    setTogglingAuto(true);
    try {
      const updated = await updateProjectMutation(selectedProject.id, {
        auto: !selectedProject.auto,
      });
      setSelectedProject(updated);
    } catch (err) {
      setAutomationActionError(
        err instanceof Error ? err.message : 'Could not change automation for this project'
      );
    } finally {
      setTogglingAuto(false);
    }
  };

  const handleResumeAutomation = async () => {
    if (!isAuthenticated || !selectedProject) return;
    setAutomationActionError(null);
    try {
      const updated = await resumeAutomation();
      setSelectedProject(updated);
    } catch (err) {
      setAutomationActionError(
        err instanceof Error ? err.message : 'Could not resume automation for this project'
      );
    }
  };

  const handleDeleteProject = async () => {
    if (!isAuthenticated || !selectedProject) return;

    setSubmitting(true);
    setOperationError(null);
    try {
      await deleteProjectMutation(selectedProject.id);
      setSelectedProject(null);
      setShowDeleteConfirm(false);
    } catch (err) {
      failed(err, 'Failed to delete project');
    } finally {
      setSubmitting(false);
    }
  };

  const handleLinkSource = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !selectedProject || !formSourceId) return;

    setSubmitting(true);
    setOperationError(null);
    try {
      const updated = await client.linkSource(selectedProject.id, formSourceId);
      setSelectedProject(updated);
      setShowSourceModal(false);
      setFormSourceId('');
    } catch (err) {
      failed(err, 'Failed to link source');
    } finally {
      setSubmitting(false);
    }
  };

  const handleUnlinkSource = async () => {
    if (!isAuthenticated || !selectedProject) return;

    setOperationError(null);
    try {
      const updated = await client.unlinkSource(selectedProject.id);
      setSelectedProject(updated);
    } catch (err) {
      failed(err, 'Failed to unlink source');
    }
  };

  const openEditModal = (project: Project) => {
    setFormName(project.name);
    setFormDescription(project.description || '');
    setFormStatus(project.status);
    openModal(setShowEditModal);
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
    setOperationError(null);
    try {
      await createSyncConfigMutation(request);
      setShowSyncModal(false);
      resetForm();
    } catch (err) {
      failed(err, 'Failed to add sync');
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteSyncConfig = async (configId: string) => {
    if (!isAuthenticated || !selectedProject) return;

    setOperationError(null);
    try {
      await deleteSyncConfigMutation(configId);
    } catch (err) {
      failed(err, 'Failed to remove sync');
    }
  };

  const syncState = (config: SyncConfig) =>
    config.last_synced_at
      ? `Synced ${formatDate(config.last_synced_at)}`
      : 'Configured, not yet synced';

  // Helper to get source info for display
  const getProjectSource = (project: Project) => {
    return sources.find((s) => s.id === project.source_id);
  };

  return (
    <div className="page page--workspace projects-page">
      <PageBar title="Projects" subtitle="Organize work with GitHub integration">
        <Tabs
          value={statusFilter}
          onValueChange={(v) => setStatusFilter(v as ProjectStatus | 'all')}
          className="projects-tabs"
        >
          <TabsList>
            <TabsTrigger value="all">All</TabsTrigger>
            <TabsTrigger value="active">Active</TabsTrigger>
            <TabsTrigger value="on_hold">On Hold</TabsTrigger>
            <TabsTrigger value="cancelled">Cancelled</TabsTrigger>
          </TabsList>
        </Tabs>
        <Button
          onClick={() => {
            resetForm();
            setShowCreateModal(true);
          }}
        >
          <PlusIcon />
          New project
        </Button>
      </PageBar>

      <div className="projects-workspace">
        {loading ? (
          <div className="projects-state">
            <span className="spinner" />
            Loading projects...
          </div>
        ) : error ? (
          <div className="projects-state projects-state--error">{error}</div>
        ) : projects.length === 0 ? (
          <EmptyState
            className="projects-empty"
            icon={<FolderIcon />}
            title={
              statusFilter === 'all'
                ? 'No projects yet'
                : `No ${statusLabels[statusFilter].toLowerCase()} projects`
            }
            description={
              statusFilter === 'all'
                ? 'Create your first project to get started'
                : 'Nothing in this workspace has that status'
            }
            action={
              statusFilter === 'all' ? (
                <Button
                  onClick={() => {
                    resetForm();
                    setShowCreateModal(true);
                  }}
                >
                  Create Project
                </Button>
              ) : (
                <Button variant="secondary" onClick={() => setStatusFilter('all')}>
                  Show all projects
                </Button>
              )
            }
          />
        ) : (
          <>
            <div className="projects-list-pane">
              <div className="projects-list">
                {projects.map((project) => (
                  <div
                    key={project.id}
                    className={`card--list project-card ${selectedProject?.id === project.id ? 'selected' : ''}`}
                    onClick={() => selectProject(project)}
                    onKeyDown={(e) => e.key === 'Enter' && selectProject(project)}
                    role="button"
                    tabIndex={0}
                  >
                    <div className="project-card-header">
                      <h3 className="project-name">{project.name}</h3>
                      <span className="project-card-badges">
                        {project.auto && (
                          <Badge variant="accent" data-testid="auto-badge">
                            Auto
                          </Badge>
                        )}
                        <Badge variant={statusVariants[project.status]}>
                          {statusLabels[project.status]}
                        </Badge>
                      </span>
                    </div>
                    <p className="project-description">{project.description}</p>
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
                      <span>{formatDate(project.updated_at)}</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
            {selectedProject ? (
              <aside className="project-details">
                <div className="details-header">
                  <h2>{selectedProject.name}</h2>
                  <Button variant="ghost" size="icon" onClick={closeDetails} aria-label="Close">
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      width="16"
                      height="16"
                      aria-hidden="true"
                    >
                      <path d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </Button>
                </div>

                <div className="details-content">
                  {operationError && !modalOpen && (
                    <div className="form-error details-error" role="alert">
                      {operationError}
                    </div>
                  )}
                  <dl className="detail-facts">
                    <dt className="detail-label">Status</dt>
                    <dd className="detail-value">
                      <Badge variant={statusVariants[selectedProject.status]}>
                        {statusLabels[selectedProject.status]}
                      </Badge>
                    </dd>
                    <dt className="detail-label">Created</dt>
                    <dd className="detail-value">{formatDate(selectedProject.created_at)}</dd>
                    <dt className="detail-label">Updated</dt>
                    <dd className="detail-value">{formatDate(selectedProject.updated_at)}</dd>
                    <dt className="detail-label">Automation</dt>
                    <dd className="detail-value">
                      <button
                        type="button"
                        className="auto-toggle"
                        aria-pressed={!!selectedProject.auto}
                        data-testid="auto-toggle"
                        disabled={togglingAuto}
                        onClick={handleToggleAuto}
                        title={
                          selectedProject.auto
                            ? 'Stop running the tasks of this project on their own'
                            : 'Run, review and merge every task of this project on its own'
                        }
                      >
                        <span className="auto-toggle-track" aria-hidden="true">
                          <span className="auto-toggle-thumb" />
                        </span>
                        Auto
                      </button>
                    </dd>
                    {selectedProject.description && (
                      <>
                        <dt className="detail-label">Description</dt>
                        <dd className="detail-value">{selectedProject.description}</dd>
                      </>
                    )}
                    <dt className="detail-label">Source</dt>
                    <dd className="detail-value">
                      {(() => {
                        const source = getProjectSource(selectedProject);
                        return source ? (
                          <div className="source-detail">
                            <span className={`source-type-badge ${source.source_type}`}>
                              {source.source_type}
                            </span>
                            <span className="source-name">{source.name}</span>
                            <a
                              href={source.url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="source-url"
                            >
                              {source.url}
                            </a>
                            <Button variant="secondary" size="sm" onClick={handleUnlinkSource}>
                              Unlink
                            </Button>
                          </div>
                        ) : (
                          <div className="source-detail">
                            <span className="no-source">No source</span>
                            <Button
                              variant="secondary"
                              size="sm"
                              onClick={() => {
                                setFormSourceId('');
                                openModal(setShowSourceModal);
                              }}
                            >
                              Link Source
                            </Button>
                          </div>
                        );
                      })()}
                    </dd>
                  </dl>

                  {automationActionError && (
                    <p className="field-error" role="alert" data-testid="automation-action-error">
                      {automationActionError}
                    </p>
                  )}

                  {selectedProject.auto && (
                    <AutomationPanel
                      automation={automation}
                      loading={automationLoading}
                      error={automationError}
                      resuming={resuming}
                      onResume={handleResumeAutomation}
                    />
                  )}

                  <div className="sync-config-section">
                    <div className="sync-config-header">
                      <h3>External Sync</h3>
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          resetForm();
                          openModal(setShowSyncModal);
                        }}
                      >
                        + Add Sync
                      </Button>
                    </div>

                    {syncLoading ? (
                      <div className="sync-config-empty">
                        <span className="spinner" /> Loading sync configurations…
                      </div>
                    ) : syncConfigs.length === 0 ? (
                      <div className="sync-config-empty">
                        No sync configured. Add one to point this project at a GitHub repository or
                        a Linear project.
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
                                <span className="sync-external-link">
                                  {config.external_project_id}
                                </span>
                              )}
                              <Button
                                variant="ghost"
                                size="sm"
                                className="sync-config-remove"
                                onClick={() => handleDeleteSyncConfig(config.id)}
                              >
                                Remove
                              </Button>
                            </div>
                            <div className="sync-config-state">
                              <span
                                className={`sync-status ${config.last_synced_at ? 'synced' : ''}`}
                              >
                                {syncState(config)}
                              </span>
                              {config.webhook_path && (
                                <code
                                  className="sync-webhook"
                                  title="Register this webhook URL with the provider"
                                >
                                  {`${window.location.origin}${config.webhook_path}`}
                                </code>
                              )}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                </div>

                <div className="details-actions">
                  <Button variant="secondary" onClick={() => openEditModal(selectedProject)}>
                    Edit Project
                  </Button>
                  <Button variant="destructive" onClick={() => openModal(setShowDeleteConfirm)}>
                    Delete
                  </Button>
                </div>
              </aside>
            ) : (
              <div className="projects-detail-placeholder">
                <EmptyState
                  icon={<FolderIcon />}
                  title="Select a project"
                  description="Choose one from the list, or create a new one."
                  action={
                    <Button
                      variant="secondary"
                      onClick={() => {
                        resetForm();
                        setShowCreateModal(true);
                      }}
                    >
                      Create a project
                    </Button>
                  }
                />
              </div>
            )}
          </>
        )}
      </div>

      {/* Create Project Wizard */}
      <CreateProjectWizard
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        onCreated={handleProjectCreated}
        createProject={createProjectMutation}
        onAuto={() => setShowAutoModal(true)}
      />

      {/* Auto project: a brief, then the planner chat asks the rest */}
      <AutoProjectModal
        isOpen={showAutoModal}
        onClose={() => setShowAutoModal(false)}
        start={(request) => {
          if (!workspaceId) {
            return Promise.reject(new Error('Select a workspace first'));
          }
          return projectsApi.startAutoProject(workspaceId, request);
        }}
        onStarted={(chatId) => {
          setShowAutoModal(false);
          navigate(`/chats?id=${chatId}`);
        }}
      />

      <Modal
        isOpen={showEditModal && selectedProject !== null}
        onClose={() => setShowEditModal(false)}
        title="Edit Project"
      >
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
          {operationError && (
            <div className="form-error" role="alert">
              {operationError}
            </div>
          )}
          <div className="modal-actions">
            <Button variant="secondary" type="button" onClick={() => setShowEditModal(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={submitting} loading={submitting}>
              {submitting ? 'Saving...' : 'Save Changes'}
            </Button>
          </div>
        </form>
      </Modal>

      <Modal
        isOpen={showDeleteConfirm && selectedProject !== null}
        onClose={() => setShowDeleteConfirm(false)}
        title="Delete Project"
        size="sm"
      >
        <p>
          Are you sure you want to delete <strong>{selectedProject?.name}</strong>? This action
          cannot be undone.
        </p>
        {operationError && (
          <div className="form-error" role="alert">
            {operationError}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="secondary" type="button" onClick={() => setShowDeleteConfirm(false)}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            type="button"
            onClick={handleDeleteProject}
            disabled={submitting}
            loading={submitting}
          >
            {submitting ? 'Deleting...' : 'Delete Project'}
          </Button>
        </div>
      </Modal>

      <Modal
        isOpen={showSourceModal && selectedProject !== null}
        onClose={() => setShowSourceModal(false)}
        title="Link Source"
      >
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
              <span className="form-hint">No sources configured. Add one in the Sources page.</span>
            )}
          </div>
          {operationError && (
            <div className="form-error" role="alert">
              {operationError}
            </div>
          )}
          <div className="modal-actions">
            <Button variant="secondary" type="button" onClick={() => setShowSourceModal(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={submitting || !formSourceId} loading={submitting}>
              {submitting ? 'Linking...' : 'Link Source'}
            </Button>
          </div>
        </form>
      </Modal>

      <Modal
        isOpen={showSyncModal && selectedProject !== null}
        onClose={() => setShowSyncModal(false)}
        title="Add External Sync"
      >
        <form onSubmit={handleCreateSyncConfig}>
          <div className="form-row">
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
          {operationError && (
            <div className="form-error" role="alert">
              {operationError}
            </div>
          )}
          <div className="modal-actions">
            <Button variant="secondary" type="button" onClick={() => setShowSyncModal(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={submitting} loading={submitting}>
              {submitting ? 'Adding...' : 'Add Sync Config'}
            </Button>
          </div>
        </form>
      </Modal>
    </div>
  );
}
