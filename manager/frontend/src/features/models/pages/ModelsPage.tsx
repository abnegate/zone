import DOMPurify from 'dompurify';
import { type FormEvent, useEffect, useState } from 'react';
import { modelsApi } from '../../../api/models';
import { Button, Modal, Tabs, TabsList, TabsTrigger, Badge, EmptyState } from '@zone/ui';
import VirtualBrowseList from '../components/VirtualBrowseList';
import { useBrowse } from '../hooks/useBrowse';
import { useModels } from '../hooks/useModels';
import { usePull } from '../hooks/usePull';
import type { BrowseModel, InstalledModel } from '../types';
import { formatNumber } from '../utils/formatters';
import './ModelsPage.css';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString();
}

type Tab = 'installed' | 'browse';

export default function ModelsPage() {
  const { models, loading: modelsLoading, error: modelsError, refresh, deleteModel } = useModels();
  const browse = useBrowse();
  const pull = usePull();

  const [activeTab, setActiveTab] = useState<Tab>('installed');
  const [modelInput, setModelInput] = useState('');
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [detailsModel, setDetailsModel] = useState<InstalledModel | BrowseModel | null>(null);
  const [modelCard, setModelCard] = useState<string | null>(null);
  const [modelCardLoading, setModelCardLoading] = useState(false);
  const [modelCardExpanded, setModelCardExpanded] = useState(false);
  const [modelSize, setModelSize] = useState<number | null>(null);
  const [browseInitialized, setBrowseInitialized] = useState(false);

  // Initial browse load - when switching to browse tab
  useEffect(() => {
    if (activeTab === 'browse' && !browseInitialized) {
      browse.search();
      setBrowseInitialized(true);
    }
  }, [activeTab, browseInitialized, browse]);

  const handlePull = async (e: FormEvent) => {
    e.preventDefault();
    if (!modelInput.trim() || pull.pulling) return;

    const success = await pull.pull(modelInput.trim());
    if (success) {
      setModelInput('');
      refresh();
    }
  };

  const handleDelete = async (name: string) => {
    setDeleting(name);
    await deleteModel(name);
    setDeleting(null);
    setDeleteConfirm(null);
    if (detailsModel && 'name' in detailsModel && detailsModel.name === name) {
      setDetailsModel(null);
    }
  };

  const handleInstall = async (model: BrowseModel) => {
    const name = model.install_name ?? model.name;
    setModelInput(name);
    setDetailsModel(null);
    const success = await pull.pull(name);
    if (success) {
      setModelInput('');
      refresh();
    }
  };

  const handleShowDetails = async (model: InstalledModel | BrowseModel) => {
    setDetailsModel(model);
    setModelCard(null);
    setModelCardExpanded(false);
    setModelSize(null);

    // Load model info for remote sources (HuggingFace, ModelScope)
    if (
      !isInstalledModel(model) &&
      (browse.source === 'huggingface' || browse.source === 'modelscope')
    ) {
      setModelCardLoading(true);
      try {
        // Prefix with source for proper routing
        const modelId = browse.source === 'modelscope' ? `modelscope/${model.id}` : model.id;
        const info = await modelsApi.getModelInfo(modelId);
        setModelCard(info.content);
        setModelSize(info.gguf_size);
      } catch {
        setModelCard(null);
        setModelSize(null);
      } finally {
        setModelCardLoading(false);
      }
    }
  };

  const handleSearch = (e: FormEvent) => {
    e.preventDefault();
    browse.search();
  };

  const isInstalledModel = (model: InstalledModel | BrowseModel): model is InstalledModel => {
    return 'size' in model && 'modified_at' in model;
  };

  return (
    <div className="page models-page">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold text-foreground">Models</h1>
        <p className="text-muted-foreground mt-1">Manage your Ollama models</p>
      </header>

      {/* Main Tabs */}
      <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as Tab)} className="mb-6">
        <TabsList>
          <TabsTrigger value="installed" className="gap-2">
            Installed
            {models.length > 0 && <Badge variant="secondary">{models.length}</Badge>}
          </TabsTrigger>
          <TabsTrigger value="browse">Browse</TabsTrigger>
        </TabsList>
      </Tabs>

      {/* Installed Tab Content */}
      {activeTab === 'installed' && (
        <>
          {/* Add Model Section */}
          <section className="card">
            <h2>Add Model</h2>
            <p className="help-text">
              Enter an Ollama model name (e.g., llama3.2) or HuggingFace GGUF model
            </p>

            <form className="model-form" onSubmit={handlePull}>
              <div className="input-group">
                <input
                  type="text"
                  placeholder="Model name..."
                  value={modelInput}
                  onChange={(e) => setModelInput(e.target.value)}
                  disabled={pull.pulling}
                />
                <Button
                  type="submit"
                  loading={pull.pulling}
                  disabled={!modelInput.trim()}
                >
                  {pull.pulling ? 'Installing...' : 'Install'}
                </Button>
              </div>
            </form>

            {/* Pull Progress */}
            {(pull.pulling || pull.result) && (
              <div className="progress-section">
                <div className="progress-header">
                  {pull.pulling
                    ? 'Installing model...'
                    : pull.result?.success
                      ? 'Installation complete'
                      : 'Installation failed'}
                </div>

                {pull.progress !== null && (
                  <div className="progress-bar-container">
                    <div className="progress-bar" style={{ width: `${pull.progress}%` }} />
                    <span className="progress-text">{Math.round(pull.progress)}%</span>
                  </div>
                )}

                {pull.steps.length > 0 && (
                  <div className="steps-list">
                    {pull.steps.map((step) => (
                      <div
                        key={`${step.name}-${step.status}`}
                        className={`step-item step-${step.status}`}
                      >
                        <span className="step-icon">
                          {step.status === 'success' ? '✓' : step.status === 'error' ? '✗' : '○'}
                        </span>
                        <span className="step-name">{step.name}</span>
                        <span className="step-message">{step.message}</span>
                      </div>
                    ))}
                  </div>
                )}

                {pull.result && (
                  <div
                    className={`result-message ${pull.result.success ? 'result-success' : 'result-error'}`}
                  >
                    {pull.result.message}
                  </div>
                )}
              </div>
            )}
          </section>

          {/* Installed Models Section */}
          <section className="card">
            <div className="card-header">
              <h2>Installed Models</h2>
              <Button variant="ghost" size="icon" onClick={refresh} title="Refresh">
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  width="16"
                  height="16"
                  aria-hidden="true"
                >
                  <path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                </svg>
              </Button>
            </div>

            {modelsLoading ? (
              <div className="loading-placeholder">
                <span className="spinner" /> Loading models...
              </div>
            ) : modelsError ? (
              <EmptyState
                icon={
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    width="48"
                    height="48"
                    className="text-destructive"
                  >
                    <path d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                  </svg>
                }
                title="Cannot connect to Ollama"
                description="Unable to fetch models. Make sure Ollama is running and accessible."
                action={<Button onClick={refresh} variant="secondary">Retry</Button>}
              />
            ) : models.length === 0 ? (
              <EmptyState
                icon={
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    width="48"
                    height="48"
                  >
                    <path d="M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z" />
                  </svg>
                }
                title="No models installed"
                description="Browse and install models to get started"
                action={<Button onClick={() => setActiveTab('browse')}>Browse Models</Button>}
              />
            ) : (
              <div className="models-list">
                {models.map((model) => (
                  <div
                    key={model.name}
                    className={`model-item ${deleting === model.name ? 'deleting' : ''}`}
                    onClick={() => handleShowDetails(model)}
                    onKeyDown={(e) => e.key === 'Enter' && handleShowDetails(model)}
                    role="button"
                    tabIndex={0}
                  >
                    <div className="model-info">
                      <span className="model-name">{model.name}</span>
                      <span className="model-meta">
                        {formatBytes(model.size)} · {formatDate(model.modified_at)}
                      </span>
                    </div>
                    <div className="model-actions">
                      <button
                        className="btn btn-danger-icon"
                        onClick={(e) => {
                          e.stopPropagation();
                          setDeleteConfirm(model.name);
                        }}
                        title="Delete model"
                        type="button"
                      >
                        <svg
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="2"
                          width="16"
                          height="16"
                          aria-hidden="true"
                        >
                          <path d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                        </svg>
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </>
      )}

      {/* Browse Tab Content */}
      {activeTab === 'browse' && (
        <section className="card">
          <div className="source-tabs">
            <button
              className={`source-tab ${browse.source === 'ollama' ? 'active' : ''}`}
              onClick={() => browse.changeSource('ollama')}
              type="button"
            >
              <svg
                viewBox="0 0 24 24"
                fill="currentColor"
                width="16"
                height="16"
                aria-hidden="true"
              >
                <circle cx="12" cy="12" r="10" />
              </svg>
              Ollama Library
            </button>
            <button
              className={`source-tab ${browse.source === 'huggingface' ? 'active' : ''}`}
              onClick={() => browse.changeSource('huggingface')}
              type="button"
            >
              <svg
                viewBox="0 0 24 24"
                fill="currentColor"
                width="16"
                height="16"
                aria-hidden="true"
              >
                <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2z" />
              </svg>
              HuggingFace
            </button>
            <button
              className={`source-tab ${browse.source === 'modelscope' ? 'active' : ''}`}
              onClick={() => browse.changeSource('modelscope')}
              type="button"
            >
              <svg
                viewBox="0 0 24 24"
                fill="currentColor"
                width="16"
                height="16"
                aria-hidden="true"
              >
                <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
              </svg>
              ModelScope
            </button>
          </div>

          <form className="search-container" onSubmit={handleSearch}>
            <input
              type="text"
              placeholder="Search models..."
              value={browse.query}
              onChange={(e) => browse.setQuery(e.target.value)}
            />
            <Button type="submit" variant="secondary">
              Search
            </Button>
          </form>

          {browse.loading ? (
            <div className="loading-placeholder">
              <span className="spinner" /> Loading...
            </div>
          ) : browse.error ? (
            <div className="error-placeholder">{browse.error}</div>
          ) : (
            <VirtualBrowseList
              models={browse.models}
              onItemClick={handleShowDetails}
              onInstall={handleInstall}
              hasMore={browse.hasMore}
              loadingMore={browse.loadingMore}
              onLoadMore={browse.loadMore}
            />
          )}
        </section>
      )}

      {/* Delete Confirmation Modal */}
      <Modal
        isOpen={deleteConfirm !== null}
        onClose={() => setDeleteConfirm(null)}
        title="Delete Model"
      >
        <p>
          Are you sure you want to delete <strong>{deleteConfirm}</strong>?
        </p>
        <p className="help-text">This action cannot be undone.</p>
        <div className="modal-actions">
          <Button variant="secondary" onClick={() => setDeleteConfirm(null)}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => deleteConfirm && handleDelete(deleteConfirm)}
            loading={deleting !== null}
          >
            {deleting === deleteConfirm ? 'Deleting...' : 'Delete'}
          </Button>
        </div>
      </Modal>

      {/* Model Details Modal */}
      {detailsModel && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setDetailsModel(null)}
            onKeyDown={(e) => e.key === 'Escape' && setDetailsModel(null)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content modal-details">
            <button
              className="modal-close"
              onClick={() => setDetailsModel(null)}
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
                aria-hidden="true"
              >
                <path d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>

            <div className="modal-details-header">
              <h3>{isInstalledModel(detailsModel) ? detailsModel.name : detailsModel.name}</h3>
              <span className="details-source">
                {isInstalledModel(detailsModel) ? 'Installed' : browse.source}
              </span>
            </div>

            {isInstalledModel(detailsModel) ? (
              <>
                <div className="details-meta">
                  <div className="details-meta-item">
                    <span className="details-label">Size</span>
                    <span>{formatBytes(detailsModel.size)}</span>
                  </div>
                  <div className="details-meta-item">
                    <span className="details-label">Modified</span>
                    <span>{formatDate(detailsModel.modified_at)}</span>
                  </div>
                  {detailsModel.details?.family && (
                    <div className="details-meta-item">
                      <span className="details-label">Family</span>
                      <span>{detailsModel.details.family}</span>
                    </div>
                  )}
                </div>
                <div className="modal-actions">
                  <Button
                    variant="destructive"
                    onClick={() => {
                      setDetailsModel(null);
                      setDeleteConfirm(detailsModel.name);
                    }}
                  >
                    Delete Model
                  </Button>
                </div>
              </>
            ) : (
              <>
                <p className="details-description">{detailsModel.description}</p>

                {detailsModel.author && (
                  <div className="details-author">
                    <span className="details-label">Author</span>
                    <span className="details-author-link">{detailsModel.author}</span>
                  </div>
                )}

                <div className="details-stats">
                  <div className="details-stat">
                    <span className="details-stat-value">
                      {formatNumber(detailsModel.downloads)}
                    </span>
                    <span className="details-stat-label">Downloads</span>
                  </div>
                  {detailsModel.likes != null && (
                    <div className="details-stat">
                      <span className="details-stat-value">{formatNumber(detailsModel.likes)}</span>
                      <span className="details-stat-label">Likes</span>
                    </div>
                  )}
                  {modelSize && (
                    <div className="details-stat">
                      <span className="details-stat-value">{formatBytes(modelSize)}</span>
                      <span className="details-stat-label">Size</span>
                    </div>
                  )}
                </div>

                {detailsModel.tags.length > 0 && (
                  <div className="details-tags">
                    {detailsModel.tags.map((tag) => (
                      <span key={tag} className="tag">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}

                {detailsModel.install_name && (
                  <div className="details-install">
                    <span className="details-label">Install command</span>
                    <code>{detailsModel.install_name}</code>
                  </div>
                )}

                {modelCard !== null && (
                  <div className="details-card">
                    <div className="details-card-header">
                      <span className="details-label">Model Card</span>
                      <button
                        className={`details-card-toggle ${modelCardExpanded ? 'expanded' : ''}`}
                        onClick={() => setModelCardExpanded(!modelCardExpanded)}
                        type="button"
                      >
                        {modelCardExpanded ? 'Collapse' : 'Expand'}
                        <svg
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="2"
                          width="12"
                          height="12"
                          aria-hidden="true"
                        >
                          <path d="M19 9l-7 7-7-7" />
                        </svg>
                      </button>
                    </div>
                    {modelCardExpanded && (
                      <div
                        className="details-card-content details-card-text"
                        // biome-ignore lint/security/noDangerouslySetInnerHtml: Content is sanitized with DOMPurify
                        dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(modelCard) }}
                      />
                    )}
                  </div>
                )}

                {modelCardLoading && (
                  <div className="details-card-loading">
                    <span className="spinner" /> Loading model card...
                  </div>
                )}

                {detailsModel.url && (
                  <div className="details-link">
                    <a href={detailsModel.url} target="_blank" rel="noopener noreferrer">
                      View on {browse.source === 'modelscope' ? 'ModelScope' : 'HuggingFace'}
                      <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        width="14"
                        height="14"
                        aria-hidden="true"
                      >
                        <path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6M15 3h6v6M10 14L21 3" />
                      </svg>
                    </a>
                  </div>
                )}

                <div className="modal-actions">
                  <Button onClick={() => handleInstall(detailsModel)}>
                    Install Model
                  </Button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
