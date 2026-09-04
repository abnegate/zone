import { Badge, Button, EmptyState, Modal, Select, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import DOMPurify from 'dompurify';
import { type FormEvent, useEffect, useState } from 'react';
import { modelsApi } from '../../../api/models';
import Capabilities from '../components/Capabilities';
import VirtualBrowseList from '../components/VirtualBrowseList';
import { useBrowse } from '../hooks/useBrowse';
import { useModels } from '../hooks/useModels';
import { usePull } from '../hooks/usePull';
import type { BrowseModel, InstalledModel, ModelSort } from '../types';
import { MODEL_FAMILY_FILTERS, MODEL_SIZE_FILTERS, MODEL_SORT_OPTIONS } from '../types';
import {
  defaultDownloadName,
  formatBytes,
  formatContextLength,
  formatDate,
  formatDownloadSizeLabel,
  formatNumber,
  modelDownloadSizes,
} from '../utils';
import './ModelsPage.css';

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
  const [selectedSize, setSelectedSize] = useState('');
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

  const isInstalledModel = (model: InstalledModel | BrowseModel): model is InstalledModel => {
    // InstalledModel has required modified_at, BrowseModel has optional
    return typeof model.modified_at === 'string' && model.modified_at.length > 0;
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

  const handleInstall = async (model: BrowseModel, pullName?: string) => {
    const sizes = modelDownloadSizes(model);
    if (sizes.length > 1 && !pullName) {
      void handleShowDetails(model);
      return;
    }
    const name = pullName || defaultDownloadName(model);
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
    setSelectedSize(isInstalledModel(model) ? '' : defaultDownloadName(model));

    // Load model info for remote sources (HuggingFace only - has README content)
    // Check the model's source when in "all" mode, otherwise check browse.source
    const modelSource = !isInstalledModel(model) && model.source ? model.source : browse.source;
    if (!isInstalledModel(model) && modelSource === 'huggingface') {
      setModelCardLoading(true);
      try {
        const modelId = model.name;
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

  return (
    <div className="page page--workspace models-page">
      <header className="models-header">
        <div className="models-header-copy">
          <h1>Models</h1>
          <p>Manage your Ollama models</p>
        </div>
        <Tabs
          value={activeTab}
          onValueChange={(v) => setActiveTab(v as Tab)}
          className="models-tabs"
        >
          <TabsList>
            <TabsTrigger value="installed" className="gap-2">
              Installed
              {models.length > 0 && <Badge variant="secondary">{models.length}</Badge>}
            </TabsTrigger>
            <TabsTrigger value="browse">Browse</TabsTrigger>
          </TabsList>
        </Tabs>
      </header>

      <div className="models-body">
        {/* Installed Tab Content */}
        {activeTab === 'installed' && (
          <>
            {/* Add Model Section */}
            <section className="card models-install-panel">
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
                  <Button type="submit" loading={pull.pulling} disabled={!modelInput.trim()}>
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
            <section className="card models-list-panel">
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
                  title={
                    modelsError.includes('401')
                      ? 'Authentication required'
                      : 'Cannot connect to Ollama'
                  }
                  description={
                    modelsError.includes('401')
                      ? 'Please log in to view installed models.'
                      : 'Unable to fetch models. Make sure Ollama is running and accessible.'
                  }
                  action={
                    <Button onClick={refresh} variant="secondary">
                      Retry
                    </Button>
                  }
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
                          {[
                            formatBytes(model.size),
                            model.details?.parameter_size,
                            model.details?.quantization_level,
                            formatDate(model.modified_at),
                          ]
                            .filter(Boolean)
                            .join(' · ')}
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
          <section className="card models-browse-panel">
            <Tabs
              value={browse.source}
              onValueChange={(v) => browse.changeSource(v as typeof browse.source)}
              className="mb-4"
            >
              <TabsList>
                <TabsTrigger value="all">All</TabsTrigger>
                <TabsTrigger value="ollama">Ollama</TabsTrigger>
                <TabsTrigger value="huggingface">HuggingFace</TabsTrigger>
                <TabsTrigger value="gpt4all">GPT4All</TabsTrigger>
                <TabsTrigger value="openrouter">OpenRouter</TabsTrigger>
              </TabsList>
            </Tabs>

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

            <div className="browse-controls">
              <label className="browse-sort">
                <span>Sort</span>
                <select
                  aria-label="Sort models"
                  value={browse.sort}
                  onChange={(e) => browse.setSort(e.target.value as ModelSort)}
                >
                  {MODEL_SORT_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>

              <div className="browse-filter-groups">
                <div className="filter-pills" role="group" aria-label="Filter by family">
                  {MODEL_FAMILY_FILTERS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={`filter-pill ${browse.family === option.value ? 'active' : ''}`}
                      aria-pressed={browse.family === option.value}
                      onClick={() => browse.setFamily(option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>

                <div className="filter-pills" role="group" aria-label="Filter by size">
                  {MODEL_SIZE_FILTERS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={`filter-pill ${browse.size === option.value ? 'active' : ''}`}
                      aria-pressed={browse.size === option.value}
                      onClick={() => browse.setSize(option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>

              {browse.hasActiveFilters && (
                <button
                  type="button"
                  className="browse-clear-filters"
                  onClick={browse.clearFilters}
                >
                  Clear filters
                </button>
              )}
            </div>

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
      </div>

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
              <h3>
                {!isInstalledModel(detailsModel) && detailsModel.display_name
                  ? detailsModel.display_name
                  : detailsModel.name}
              </h3>
              <span className="details-source">
                {isInstalledModel(detailsModel)
                  ? 'Installed'
                  : detailsModel.source || browse.source}
              </span>
            </div>

            {isInstalledModel(detailsModel) ? (
              <>
                {detailsModel.details?.description && (
                  <p className="details-description">{detailsModel.details.description}</p>
                )}
                <div className="details-meta">
                  <div className="details-meta-item">
                    <span className="details-label">Size</span>
                    <span>{formatBytes(detailsModel.size)}</span>
                  </div>
                  {detailsModel.details?.parameter_size && (
                    <div className="details-meta-item">
                      <span className="details-label">Parameters</span>
                      <span>{detailsModel.details.parameter_size}</span>
                    </div>
                  )}
                  {detailsModel.details?.quantization_level && (
                    <div className="details-meta-item">
                      <span className="details-label">Quantization</span>
                      <span>{detailsModel.details.quantization_level}</span>
                    </div>
                  )}
                  {detailsModel.details?.family && (
                    <div className="details-meta-item">
                      <span className="details-label">Family</span>
                      <span>{detailsModel.details.family}</span>
                    </div>
                  )}
                  <div className="details-meta-item">
                    <span className="details-label">Modified</span>
                    <span>{formatDate(detailsModel.modified_at)}</span>
                  </div>
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
                {detailsModel.description && (
                  <p className="details-description">{detailsModel.description}</p>
                )}

                {(detailsModel.details?.parameter_size ||
                  detailsModel.size ||
                  modelSize ||
                  detailsModel.details?.context_length) && (
                  <div className="details-stats">
                    {detailsModel.details?.parameter_size && (
                      <div className="details-stat">
                        <span className="details-stat-value">
                          {detailsModel.details.parameter_size}
                        </span>
                        <span className="details-stat-label">Parameters</span>
                      </div>
                    )}
                    {(detailsModel.size || modelSize) && (
                      <div className="details-stat">
                        <span className="details-stat-value">
                          {formatBytes(detailsModel.size || modelSize || 0)}
                        </span>
                        <span className="details-stat-label">
                          {detailsModel.source === 'huggingface' ? 'Repo size' : 'Size'}
                        </span>
                      </div>
                    )}
                    {detailsModel.details?.context_length && (
                      <div className="details-stat">
                        <span className="details-stat-value">
                          {formatContextLength(detailsModel.details.context_length)}
                        </span>
                        <span className="details-stat-label">Context</span>
                      </div>
                    )}
                  </div>
                )}

                {(detailsModel.details?.family ||
                  detailsModel.details?.quantization_level ||
                  detailsModel.details?.format ||
                  detailsModel.details?.license ||
                  detailsModel.details?.ram_required_gb ||
                  detailsModel.author ||
                  detailsModel.downloads != null ||
                  detailsModel.likes != null ||
                  detailsModel.modified_at) && (
                  <div className="details-meta">
                    {detailsModel.details?.family && (
                      <div className="details-meta-item">
                        <span className="details-label">Family</span>
                        <span>{detailsModel.details.family}</span>
                      </div>
                    )}
                    {detailsModel.details?.quantization_level && (
                      <div className="details-meta-item">
                        <span className="details-label">Quantization</span>
                        <span>{detailsModel.details.quantization_level}</span>
                      </div>
                    )}
                    {detailsModel.details?.format && (
                      <div className="details-meta-item">
                        <span className="details-label">Format</span>
                        <span>{detailsModel.details.format}</span>
                      </div>
                    )}
                    {detailsModel.details?.license && (
                      <div className="details-meta-item">
                        <span className="details-label">License</span>
                        <span>{detailsModel.details.license}</span>
                      </div>
                    )}
                    {detailsModel.details?.ram_required_gb && (
                      <div className="details-meta-item">
                        <span className="details-label">RAM</span>
                        <span>{detailsModel.details.ram_required_gb} GB</span>
                      </div>
                    )}
                    {detailsModel.author && (
                      <div className="details-meta-item details-author">
                        <span className="details-label">Author</span>
                        <span>{detailsModel.author}</span>
                      </div>
                    )}
                    {detailsModel.downloads != null && (
                      <div className="details-meta-item">
                        <span className="details-label">
                          {detailsModel.source === 'ollama' ? 'Pulls' : 'Downloads'}
                        </span>
                        <span>{formatNumber(detailsModel.downloads)}</span>
                      </div>
                    )}
                    {detailsModel.likes != null && (
                      <div className="details-meta-item">
                        <span className="details-label">Likes</span>
                        <span>{formatNumber(detailsModel.likes)}</span>
                      </div>
                    )}
                    {detailsModel.modified_at && (
                      <div className="details-meta-item">
                        <span className="details-label">Updated</span>
                        <span>{formatDate(detailsModel.modified_at)}</span>
                      </div>
                    )}
                  </div>
                )}

                <div className="details-use-cases">
                  <span className="details-label">Capabilities</span>
                  <Capabilities capabilities={detailsModel.capabilities} />
                </div>

                {detailsModel.tags && detailsModel.tags.length > 0 && (
                  <div className="details-tags">
                    {detailsModel.tags.slice(0, 8).map((tag) => (
                      <span key={tag} className="tag">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}

                {detailsModel.url && (
                  <div className="details-link">
                    <a href={detailsModel.url} target="_blank" rel="noreferrer">
                      View source
                    </a>
                  </div>
                )}

                {modelDownloadSizes(detailsModel).length > 1 && (
                  <div className="details-size-picker">
                    <Select
                      label="Size"
                      value={selectedSize}
                      onValueChange={setSelectedSize}
                      helpText="This model is published in more than one size."
                      options={modelDownloadSizes(detailsModel).map((option) => ({
                        value: option.name,
                        label: formatDownloadSizeLabel(option),
                      }))}
                    />
                  </div>
                )}

                <div className="details-install">
                  <span className="details-label">Install command</span>
                  <code>{selectedSize || detailsModel.name}</code>
                </div>

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

                <div className="modal-actions">
                  <Button
                    onClick={() =>
                      handleInstall(detailsModel, selectedSize || defaultDownloadName(detailsModel))
                    }
                  >
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
