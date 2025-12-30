// Manager Frontend Application

// =============================================================================
// Authentication
// =============================================================================

const API_KEY_STORAGE_KEY = 'manager_api_key';

function getApiKey() {
    return localStorage.getItem(API_KEY_STORAGE_KEY) || '';
}

function setApiKey(key) {
    localStorage.setItem(API_KEY_STORAGE_KEY, key);
}

function clearApiKey() {
    localStorage.removeItem(API_KEY_STORAGE_KEY);
}

function getAuthHeaders() {
    const apiKey = getApiKey();
    return apiKey ? { 'Authorization': 'Bearer ' + apiKey } : {};
}

async function checkAuth() {
    const apiKey = getApiKey();
    if (!apiKey) {
        showLoginOverlay();
        return false;
    }

    // Verify the API key by making a test request
    try {
        const response = await fetch('/api/models', {
            headers: getAuthHeaders()
        });

        if (response.status === 401) {
            clearApiKey();
            showLoginOverlay();
            return false;
        }

        hideLoginOverlay();
        return true;
    } catch (error) {
        showLoginOverlay();
        return false;
    }
}

function showLoginOverlay() {
    document.getElementById('login-overlay').hidden = false;
}

function hideLoginOverlay() {
    document.getElementById('login-overlay').hidden = true;
}

async function handleLogin(event) {
    event.preventDefault();

    const apiKeyInput = document.getElementById('api-key-input');
    const loginBtn = document.getElementById('login-btn');
    const loginError = document.getElementById('login-error');
    const btnText = loginBtn.querySelector('.btn-text');
    const btnLoading = loginBtn.querySelector('.btn-loading');

    const apiKey = apiKeyInput.value.trim();
    if (!apiKey) {
        loginError.textContent = 'Please enter an API key';
        loginError.hidden = false;
        return;
    }

    // Show loading state
    loginBtn.disabled = true;
    btnText.hidden = true;
    btnLoading.hidden = false;
    loginError.hidden = true;

    // Test the API key
    try {
        const response = await fetch('/api/models', {
            headers: { 'Authorization': 'Bearer ' + apiKey }
        });

        if (response.status === 401) {
            loginError.textContent = 'Invalid API key';
            loginError.hidden = false;
        } else if (response.ok) {
            // Success - store the key and proceed
            setApiKey(apiKey);
            hideLoginOverlay();
            loadModels();
            loadBrowseResults();
        } else {
            loginError.textContent = 'Authentication failed';
            loginError.hidden = false;
        }
    } catch (error) {
        loginError.textContent = 'Connection error: ' + error.message;
        loginError.hidden = false;
    } finally {
        loginBtn.disabled = false;
        btnText.hidden = false;
        btnLoading.hidden = true;
    }
}

// =============================================================================
// State
// =============================================================================

let currentSource = 'ollama';
let modelToDelete = null;
let downloadTimer = null;
let downloadStartTime = null;

// Pagination state
let browseOffset = 0;
let browseHasMore = true;
let browseIsLoading = false;
const BROWSE_LIMIT = 20;

// Model details state
let selectedModel = null;
let modelCardCache = {};
let modelCardExpanded = false;

document.addEventListener('DOMContentLoaded', async function() {
    setupLoginListener();

    // Check authentication before loading
    const isAuthenticated = await checkAuth();
    if (isAuthenticated) {
        loadModels();
        loadBrowseResults();
        setupEventListeners();
    } else {
        // Setup event listeners even when not authenticated
        // so they're ready after login
        setupEventListeners();
    }
});

function setupLoginListener() {
    const loginForm = document.getElementById('login-form');
    if (loginForm) {
        loginForm.addEventListener('submit', handleLogin);
    }
}

// =============================================================================
// Event Listeners
// =============================================================================

function setupEventListeners() {
    // Form submission
    const form = document.getElementById('add-model-form');
    form.addEventListener('submit', handleAddModel);

    // Refresh button
    const refreshBtn = document.getElementById('refresh-btn');
    refreshBtn.addEventListener('click', loadModels);

    // Source tabs
    const sourceTabs = document.querySelectorAll('.source-tab');
    sourceTabs.forEach(tab => {
        tab.addEventListener('click', function() {
            sourceTabs.forEach(t => t.classList.remove('active'));
            this.classList.add('active');
            currentSource = this.dataset.source;
            resetBrowsePagination();
            loadBrowseResults();
        });
    });

    // Search
    const searchBtn = document.getElementById('search-btn');
    searchBtn.addEventListener('click', function() {
        resetBrowsePagination();
        loadBrowseResults();
    });

    const searchInput = document.getElementById('browse-search');
    searchInput.addEventListener('keypress', function(e) {
        if (e.key === 'Enter') {
            resetBrowsePagination();
            loadBrowseResults();
        }
    });

    // Infinite scroll
    setupInfiniteScroll();

    // Clear result on input focus
    const modelInput = document.getElementById('model-input');
    modelInput.addEventListener('focus', function() {
        hideResult();
    });

    // Delete modal
    const deleteCancel = document.getElementById('delete-cancel');
    deleteCancel.addEventListener('click', hideDeleteModal);

    const deleteConfirm = document.getElementById('delete-confirm');
    deleteConfirm.addEventListener('click', confirmDelete);

    const modalBackdrop = document.querySelector('#delete-modal .modal-backdrop');
    if (modalBackdrop) {
        modalBackdrop.addEventListener('click', hideDeleteModal);
    }

    // Details modal
    const detailsClose = document.getElementById('details-close');
    detailsClose.addEventListener('click', hideDetailsModal);

    const detailsCancel = document.getElementById('details-cancel');
    detailsCancel.addEventListener('click', hideDetailsModal);

    const detailsInstall = document.getElementById('details-install');
    detailsInstall.addEventListener('click', installFromDetails);

    const detailsBackdrop = document.querySelector('#model-details-modal .modal-backdrop');
    if (detailsBackdrop) {
        detailsBackdrop.addEventListener('click', hideDetailsModal);
    }

    // Model card toggle
    const cardToggle = document.getElementById('details-card-toggle');
    cardToggle.addEventListener('click', toggleModelCard);
}

// =============================================================================
// Add Model
// =============================================================================

async function handleAddModel(event) {
    event.preventDefault();

    const modelInput = document.getElementById('model-input');
    const modelName = modelInput.value.trim();

    if (!modelName) {
        showResult('error', 'Please enter a model name');
        return;
    }

    setLoading(true);
    hideResult();
    showProgress();
    clearSteps();
    setProgressTitle('Adding model: ' + modelName);

    // Start download progress with WebSocket streaming
    startStreamingDownload(modelName, modelInput);
}

// =============================================================================
// Streaming Download with WebSocket
// =============================================================================

let websocket = null;
let currentModelInput = null;

function startStreamingDownload(modelName, modelInput) {
    downloadStartTime = Date.now();
    currentModelInput = modelInput;

    // Add progress UI
    addProgressStep();

    // Start timer update
    downloadTimer = setInterval(updateDownloadTime, 1000);

    // Determine WebSocket URL (ws:// or wss://) with API key auth
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const apiKey = getApiKey();
    const wsUrl = protocol + '//' + window.location.host + '/ws/pull?api_key=' + encodeURIComponent(apiKey);

    // Close any existing connection
    if (websocket) {
        websocket.close();
    }

    // Create WebSocket connection
    websocket = new WebSocket(wsUrl);

    websocket.onopen = function() {
        // Send the model name to start the pull
        websocket.send(JSON.stringify({ model: modelName }));
    };

    websocket.onmessage = function(event) {
        handleWebSocketMessage(event.data);
    };

    websocket.onerror = function(error) {
        // Check if this might be an auth error
        handleDownloadError('WebSocket connection failed - please check your API key', currentModelInput);
    };

    websocket.onclose = function(event) {
        // Connection closed - if not a clean close, show error
        if (!event.wasClean && downloadStartTime) {
            handleDownloadError('Connection closed unexpectedly', currentModelInput);
        }
    };
}

function handleWebSocketMessage(data) {
    try {
        const message = JSON.parse(data);

        switch (message.type) {
            case 'progress':
                updateProgressUI(message);
                break;
            case 'step':
                addStep(message.step, message.success, message.message);
                break;
            case 'complete':
                finishDownload(currentModelInput, message);
                closeWebSocket();
                break;
            case 'error':
                handleDownloadError(message.message || 'Unknown error', currentModelInput);
                closeWebSocket();
                break;
        }
    } catch (e) {
        console.error('Failed to parse WebSocket message:', e);
    }
}

function closeWebSocket() {
    if (websocket) {
        websocket.close();
        websocket = null;
    }
}

function updateProgressUI(progress) {
    const statusEl = document.getElementById('download-status');
    const progressBar = document.getElementById('download-progress-bar');

    if (statusEl && progress.status) {
        statusEl.textContent = progress.status;
    }

    if (progressBar) {
        if (progress.percent !== null && progress.percent !== undefined) {
            progressBar.classList.remove('progress-bar-animated');
            progressBar.style.width = progress.percent + '%';
        } else {
            // Indeterminate progress
            progressBar.classList.add('progress-bar-animated');
        }
    }

    // Show bytes info if available
    if (progress.completed && progress.total) {
        const statusEl = document.getElementById('download-status');
        if (statusEl) {
            const completed = formatSize(progress.completed);
            const total = formatSize(progress.total);
            statusEl.textContent = progress.status + ' (' + completed + ' / ' + total + ')';
        }
    }
}

function updateDownloadTime() {
    if (!downloadStartTime) return;

    const elapsed = Math.floor((Date.now() - downloadStartTime) / 1000);
    const minutes = Math.floor(elapsed / 60);
    const seconds = elapsed % 60;
    const timeStr = minutes + ':' + seconds.toString().padStart(2, '0');

    const timeEl = document.getElementById('download-time');
    if (timeEl) {
        timeEl.textContent = timeStr;
    }
}

function finishDownload(modelInput, result) {
    stopDownloadProgress();

    if (result && result.success) {
        showResult('success', result.message || 'Model added successfully');
        modelInput.value = '';
        setTimeout(loadModels, 500);
    } else if (result) {
        showResult('error', result.message || 'Failed to add model');
    }

    setLoading(false);
}

function handleDownloadError(message, modelInput) {
    stopDownloadProgress();
    closeWebSocket();
    showResult('error', message);
    setLoading(false);
}

function stopDownloadProgress() {
    if (downloadTimer) {
        clearInterval(downloadTimer);
        downloadTimer = null;
    }
    downloadStartTime = null;
    currentModelInput = null;

    // Mark progress step as complete
    const progressStep = document.getElementById('download-progress-step');
    if (progressStep) {
        const progressBar = progressStep.querySelector('.progress-bar-fill');
        if (progressBar) {
            progressBar.classList.remove('progress-bar-animated');
            progressBar.style.width = '100%';
        }
    }
}

function addProgressStep() {
    const stepsList = document.getElementById('steps-list');

    const stepHtml = `
        <div class="step-item step-progress" id="download-progress-step">
            <span class="step-icon">
                <span class="spinner-small"></span>
            </span>
            <span class="step-name">Download</span>
            <span class="step-message">
                <span id="download-status">Connecting to Ollama...</span>
                <span id="download-time" class="download-time">0:00</span>
            </span>
            <div class="progress-bar-container">
                <div class="progress-bar-fill progress-bar-animated" id="download-progress-bar"></div>
            </div>
        </div>
    `;

    stepsList.innerHTML = stepHtml;
}

// =============================================================================
// Load Installed Models
// =============================================================================

async function loadModels() {
    const modelsList = document.getElementById('models-list');
    modelsList.innerHTML = '<div class="loading-placeholder">Loading models...</div>';

    try {
        const response = await fetch('/api/models', {
            headers: getAuthHeaders()
        });

        // Handle auth errors
        if (response.status === 401) {
            clearApiKey();
            showLoginOverlay();
            return;
        }

        const data = await response.json();

        if (data.error) {
            modelsList.innerHTML = '<div class="error-placeholder">Error: ' + escapeHtml(data.error) + '</div>';
            return;
        }

        if (!data.models || data.models.length === 0) {
            modelsList.innerHTML = '<div class="empty-placeholder">No models installed yet</div>';
            return;
        }

        const models = data.models.sort((a, b) => a.name.localeCompare(b.name));

        // Store for details modal
        models.forEach(model => {
            installedModels[model.name] = model;
        });

        modelsList.innerHTML = models.map(model => {
            const size = formatSize(model.size);
            const modified = formatDate(model.modified_at);

            return `
                <div class="model-item" data-model="${escapeHtml(model.name)}" onclick="showInstalledModelDetails('${escapeHtml(model.name)}')">
                    <div class="model-info">
                        <span class="model-name">${escapeHtml(model.name)}</span>
                        <span class="model-meta">${size} • ${modified}</span>
                    </div>
                    <div class="model-actions">
                        <button class="btn btn-icon btn-danger-icon" onclick="event.stopPropagation(); showDeleteModal('${escapeHtml(model.name)}')" title="Delete model">
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                                <line x1="10" y1="11" x2="10" y2="17"/>
                                <line x1="14" y1="11" x2="14" y2="17"/>
                            </svg>
                        </button>
                    </div>
                </div>
            `;
        }).join('');

    } catch (error) {
        modelsList.innerHTML = '<div class="error-placeholder">Failed to load models: ' + escapeHtml(error.message) + '</div>';
    }
}

// =============================================================================
// Delete Model
// =============================================================================

function showDeleteModal(modelName) {
    modelToDelete = modelName;
    document.getElementById('delete-model-name').textContent = modelName;
    document.getElementById('delete-modal').hidden = false;
}

function hideDeleteModal() {
    modelToDelete = null;
    document.getElementById('delete-modal').hidden = true;
}

async function confirmDelete() {
    if (!modelToDelete) return;

    const modelName = modelToDelete;
    hideDeleteModal();

    // Show loading state on the model item
    const modelItem = document.querySelector(`.model-item[data-model="${modelName}"]`);
    if (modelItem) {
        modelItem.classList.add('deleting');
    }

    try {
        const response = await fetch('/api/models/' + encodeURIComponent(modelName), {
            method: 'DELETE',
            headers: getAuthHeaders()
        });

        // Handle auth errors
        if (response.status === 401) {
            clearApiKey();
            showLoginOverlay();
            return;
        }

        const data = await response.json();

        if (data.success) {
            showResult('success', 'Model deleted successfully');
            loadModels();
        } else {
            showResult('error', data.message || 'Failed to delete model');
            if (modelItem) {
                modelItem.classList.remove('deleting');
            }
        }
    } catch (error) {
        showResult('error', 'Network error: ' + error.message);
        if (modelItem) {
            modelItem.classList.remove('deleting');
        }
    }
}

// =============================================================================
// Browse Models
// =============================================================================

function resetBrowsePagination() {
    browseOffset = 0;
    browseHasMore = true;
    browseModels = {};
}

function setupInfiniteScroll() {
    const resultsContainer = document.getElementById('browse-results');

    // Use scroll event on the container since it has overflow-y: auto
    resultsContainer.addEventListener('scroll', () => {
        if (browseIsLoading || !browseHasMore) return;

        const { scrollTop, scrollHeight, clientHeight } = resultsContainer;
        // Load more when within 100px of the bottom
        if (scrollTop + clientHeight >= scrollHeight - 100) {
            loadMoreBrowseResults();
        }
    });
}

async function loadBrowseResults() {
    const resultsContainer = document.getElementById('browse-results');
    const searchQuery = document.getElementById('browse-search').value.trim();

    browseIsLoading = true;
    resultsContainer.innerHTML = '<div class="loading-placeholder">Searching...</div>';

    try {
        const params = new URLSearchParams({
            source: currentSource,
            q: searchQuery,
            limit: BROWSE_LIMIT.toString(),
            offset: browseOffset.toString(),
        });

        const response = await fetch('/api/browse?' + params, {
            headers: getAuthHeaders()
        });

        // Handle auth errors
        if (response.status === 401) {
            clearApiKey();
            showLoginOverlay();
            return;
        }

        const data = await response.json();

        if (data.error) {
            resultsContainer.innerHTML = '<div class="error-placeholder">Error: ' + escapeHtml(data.error) + '</div>';
            browseHasMore = false;
            return;
        }

        if (!data.models || data.models.length === 0) {
            resultsContainer.innerHTML = '<div class="empty-placeholder">No models found</div>';
            browseHasMore = false;
            return;
        }

        browseHasMore = data.has_more !== false;
        browseOffset += data.models.length;

        resultsContainer.innerHTML = renderBrowseItems(data.models);

    } catch (error) {
        resultsContainer.innerHTML = '<div class="error-placeholder">Failed to load: ' + escapeHtml(error.message) + '</div>';
        browseHasMore = false;
    } finally {
        browseIsLoading = false;
    }
}

async function loadMoreBrowseResults() {
    if (!browseHasMore || browseIsLoading) return;

    const resultsContainer = document.getElementById('browse-results');
    const searchQuery = document.getElementById('browse-search').value.trim();

    browseIsLoading = true;

    // Add loading indicator at the bottom
    const loadingEl = document.createElement('div');
    loadingEl.className = 'loading-more';
    loadingEl.innerHTML = '<div class="spinner-small"></div> Loading more...';
    resultsContainer.appendChild(loadingEl);

    try {
        const params = new URLSearchParams({
            source: currentSource,
            q: searchQuery,
            limit: BROWSE_LIMIT.toString(),
            offset: browseOffset.toString(),
        });

        const response = await fetch('/api/browse?' + params, {
            headers: getAuthHeaders()
        });

        // Remove loading indicator
        loadingEl.remove();

        // Handle auth errors
        if (response.status === 401) {
            clearApiKey();
            showLoginOverlay();
            return;
        }

        const data = await response.json();

        if (data.error) {
            browseHasMore = false;
            return;
        }

        if (!data.models || data.models.length === 0) {
            browseHasMore = false;
            return;
        }

        browseHasMore = data.has_more !== false;
        browseOffset += data.models.length;

        // Append new items
        resultsContainer.innerHTML += renderBrowseItems(data.models);

    } catch (error) {
        loadingEl.remove();
        browseHasMore = false;
    } finally {
        browseIsLoading = false;
    }
}

// Store models for details modal
let browseModels = {};
let installedModels = {};

function renderBrowseItems(models) {
    return models.map(model => {
        const downloads = formatDownloads(model.downloads);
        const installName = model.install_name || model.name;
        const tags = (model.tags || []).slice(0, 3);
        const modelId = model.id || model.name;

        // Store model data for details modal
        browseModels[modelId] = model;

        return `
            <div class="browse-item" onclick="showModelDetails('${escapeHtml(modelId)}')">
                <div class="browse-info">
                    <div class="browse-header">
                        <span class="browse-name">${escapeHtml(model.name)}</span>
                        <span class="browse-downloads">${downloads} downloads</span>
                    </div>
                    <p class="browse-description">${escapeHtml(model.description || '')}</p>
                    ${tags.length > 0 ? `
                        <div class="browse-tags">
                            ${tags.map(tag => `<span class="tag">${escapeHtml(tag)}</span>`).join('')}
                        </div>
                    ` : ''}
                </div>
                <button class="btn btn-primary btn-small" onclick="event.stopPropagation(); installModel('${escapeHtml(installName)}')">
                    Install
                </button>
            </div>
        `;
    }).join('');
}

function installModel(modelName) {
    document.getElementById('model-input').value = modelName;
    document.getElementById('model-input').scrollIntoView({ behavior: 'smooth', block: 'center' });
    document.getElementById('model-input').focus();
}

// =============================================================================
// Model Details Modal
// =============================================================================

function showModelDetails(modelId) {
    const model = browseModels[modelId];
    if (!model) return;

    selectedModel = { ...model, isInstalled: false };

    // Reset button to Install state
    const installBtn = document.getElementById('details-install');
    installBtn.textContent = 'Install';
    installBtn.classList.add('btn-primary');
    installBtn.classList.remove('btn-danger');

    // Populate modal - basic info
    document.getElementById('details-name').textContent = model.name;
    document.getElementById('details-source').textContent = currentSource;
    document.getElementById('details-description').textContent = model.description || 'No description available';
    document.getElementById('details-downloads').textContent = formatDownloads(model.downloads);

    // Reset downloads label
    document.querySelector('#details-downloads').closest('.details-stat').querySelector('.details-stat-label').textContent = 'Downloads';

    // Author (HuggingFace only)
    const authorRow = document.getElementById('details-author-row');
    const authorEl = document.getElementById('details-author');
    if (model.author) {
        authorEl.textContent = model.author;
        authorEl.href = `https://huggingface.co/${model.author}`;
        authorRow.hidden = false;
    } else {
        authorRow.hidden = true;
    }

    // Likes (HuggingFace only)
    const likesRow = document.getElementById('details-likes-row');
    if (model.likes !== undefined && model.likes > 0) {
        document.getElementById('details-likes').textContent = formatDownloads(model.likes);
        likesRow.hidden = false;
    } else {
        likesRow.hidden = true;
    }

    // Meta section (pipeline tag and updated date)
    const metaRow = document.getElementById('details-meta-row');
    const pipelineRow = document.getElementById('details-pipeline-row');
    const updatedRow = document.getElementById('details-updated-row');
    let showMeta = false;

    if (model.pipeline_tag) {
        document.getElementById('details-pipeline').textContent = model.pipeline_tag;
        pipelineRow.hidden = false;
        showMeta = true;
    } else {
        pipelineRow.hidden = true;
    }

    if (model.last_modified) {
        document.getElementById('details-updated').textContent = formatDate(model.last_modified);
        updatedRow.hidden = false;
        showMeta = true;
    } else {
        updatedRow.hidden = true;
    }
    metaRow.hidden = !showMeta;

    // Tags
    const tagsContainer = document.getElementById('details-tags');
    const tags = model.tags || [];
    if (tags.length > 0) {
        tagsContainer.innerHTML = tags.map(tag => `<span class="tag">${escapeHtml(tag)}</span>`).join('');
        tagsContainer.hidden = false;
    } else {
        tagsContainer.hidden = true;
    }

    // Install name
    const installName = model.install_name || model.name;
    document.getElementById('details-install-name').textContent = installName;

    // External link (HuggingFace only)
    const linkRow = document.getElementById('details-link-row');
    const linkEl = document.getElementById('details-link');
    if (model.url) {
        linkEl.href = model.url;
        linkRow.hidden = false;
    } else {
        linkRow.hidden = true;
    }

    // Model card (HuggingFace only)
    const cardRow = document.getElementById('details-card-row');
    if (currentSource === 'huggingface' && model.id) {
        cardRow.hidden = false;
        // Reset model card state
        modelCardExpanded = false;
        document.getElementById('details-card-content').hidden = true;
        document.getElementById('details-card-toggle').classList.remove('expanded');
        document.getElementById('details-card-toggle-text').textContent = 'Show';
        document.getElementById('details-card-text').innerHTML = '';
        document.getElementById('details-card-loading').hidden = false;
    } else {
        cardRow.hidden = true;
    }

    // Show modal
    document.getElementById('model-details-modal').hidden = false;
}

function hideDetailsModal() {
    document.getElementById('model-details-modal').hidden = true;
    selectedModel = null;
    modelCardExpanded = false;
}

function showInstalledModelDetails(modelName) {
    const model = installedModels[modelName];
    if (!model) return;

    selectedModel = { ...model, isInstalled: true };

    // Populate modal
    document.getElementById('details-name').textContent = model.name;
    document.getElementById('details-source').textContent = 'installed';
    document.getElementById('details-description').textContent = model.details?.description || 'Locally installed model';

    // Size as the main stat
    document.getElementById('details-downloads').textContent = formatSize(model.size);
    document.querySelector('#details-downloads').closest('.details-stat').querySelector('.details-stat-label').textContent = 'Size';

    // Hide author (not applicable)
    document.getElementById('details-author-row').hidden = true;

    // Hide likes
    document.getElementById('details-likes-row').hidden = true;

    // Show meta with modified date and family
    const metaRow = document.getElementById('details-meta-row');
    const pipelineRow = document.getElementById('details-pipeline-row');
    const updatedRow = document.getElementById('details-updated-row');

    if (model.details?.family) {
        document.getElementById('details-pipeline').textContent = model.details.family;
        pipelineRow.hidden = false;
    } else {
        pipelineRow.hidden = true;
    }

    if (model.modified_at) {
        document.getElementById('details-updated').textContent = formatDate(model.modified_at);
        updatedRow.hidden = false;
    } else {
        updatedRow.hidden = true;
    }
    metaRow.hidden = pipelineRow.hidden && updatedRow.hidden;

    // Hide tags
    document.getElementById('details-tags').hidden = true;

    // Show model name as "installed"
    document.getElementById('details-install-name').textContent = model.name + ' (installed)';

    // Hide external link
    document.getElementById('details-link-row').hidden = true;

    // Hide model card section
    document.getElementById('details-card-row').hidden = true;

    // Change install button to delete
    const installBtn = document.getElementById('details-install');
    installBtn.textContent = 'Delete';
    installBtn.classList.remove('btn-primary');
    installBtn.classList.add('btn-danger');

    // Show modal
    document.getElementById('model-details-modal').hidden = false;
}

function installFromDetails() {
    if (!selectedModel) return;

    if (selectedModel.isInstalled) {
        // Delete installed model
        const modelName = selectedModel.name;
        hideDetailsModal();
        showDeleteModal(modelName);
    } else {
        // Install new model
        const installName = selectedModel.install_name || selectedModel.name;
        hideDetailsModal();
        installModel(installName);
    }
}

async function toggleModelCard() {
    const content = document.getElementById('details-card-content');
    const toggle = document.getElementById('details-card-toggle');
    const toggleText = document.getElementById('details-card-toggle-text');

    modelCardExpanded = !modelCardExpanded;

    if (modelCardExpanded) {
        content.hidden = false;
        toggle.classList.add('expanded');
        toggleText.textContent = 'Hide';

        // Fetch model card if not cached
        if (selectedModel && selectedModel.id) {
            await fetchModelCard(selectedModel.id);
        }
    } else {
        content.hidden = true;
        toggle.classList.remove('expanded');
        toggleText.textContent = 'Show';
    }
}

async function fetchModelCard(modelId) {
    const loadingEl = document.getElementById('details-card-loading');
    const textEl = document.getElementById('details-card-text');

    // Check cache first
    if (modelCardCache[modelId]) {
        loadingEl.hidden = true;
        textEl.innerHTML = modelCardCache[modelId];
        return;
    }

    loadingEl.hidden = false;
    textEl.innerHTML = '';

    try {
        const response = await fetch('/api/model-card/' + encodeURIComponent(modelId), {
            headers: getAuthHeaders()
        });

        // Handle auth errors
        if (response.status === 401) {
            clearApiKey();
            showLoginOverlay();
            return;
        }

        const data = await response.json();

        if (data.success && data.content) {
            const html = parseMarkdown(data.content);
            modelCardCache[modelId] = html;
            textEl.innerHTML = html;
        } else {
            textEl.innerHTML = '<p class="text-muted">Model card not available</p>';
        }
    } catch (error) {
        textEl.innerHTML = '<p class="text-muted">Failed to load model card</p>';
    } finally {
        loadingEl.hidden = true;
    }
}

function parseMarkdown(markdown) {
    // Remove YAML front matter (between --- markers)
    markdown = markdown.replace(/^---[\s\S]*?---\n*/m, '');

    // First pass: escape all HTML to prevent XSS
    markdown = escapeHtml(markdown);

    // Basic markdown to HTML conversion
    let html = markdown
        // Code blocks (before other processing)
        .replace(/```(\w*)\n([\s\S]*?)```/g, (_, lang, code) => {
            return '<pre><code>' + code + '</code></pre>';
        })
        // Inline code
        .replace(/`([^`]+)`/g, (_, code) => {
            return '<code>' + code + '</code>';
        })
        // Headers (escape content to prevent nested HTML)
        .replace(/^### (.+)$/gm, (_, content) => '<h3>' + content + '</h3>')
        .replace(/^## (.+)$/gm, (_, content) => '<h2>' + content + '</h2>')
        .replace(/^# (.+)$/gm, (_, content) => '<h1>' + content + '</h1>')
        // Bold and italic
        .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
        .replace(/\*(.+?)\*/g, '<em>$1</em>')
        // Links - sanitize URLs to prevent javascript: and data: URIs
        .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, url) => {
            const sanitizedUrl = sanitizeUrl(url);
            return '<a href="' + sanitizedUrl + '" target="_blank" rel="noopener noreferrer">' + text + '</a>';
        })
        // Images - convert to safe links
        .replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, url) => {
            const sanitizedUrl = sanitizeUrl(url);
            return '<a href="' + sanitizedUrl + '" target="_blank" rel="noopener noreferrer">[Image: ' + (alt || 'view') + ']</a>';
        })
        // Unordered lists
        .replace(/^\s*[-*]\s+(.+)$/gm, '<li>$1</li>')
        // Paragraphs (split by double newlines)
        .split(/\n\n+/)
        .map(para => {
            para = para.trim();
            if (!para) return '';
            // Don't wrap HTML tags in paragraphs
            if (para.match(/^<[a-z]/i)) {
                return para;
            }
            // Wrap list items in ul
            if (para.includes('<li>')) {
                return '<ul>' + para + '</ul>';
            }
            return '<p>' + para.replace(/\n/g, '<br>') + '</p>';
        })
        .join('\n');

    return html;
}

function sanitizeUrl(url) {
    // Remove leading/trailing whitespace
    url = url.trim();

    // Block dangerous protocols
    const lowerUrl = url.toLowerCase();
    if (lowerUrl.startsWith('javascript:') ||
        lowerUrl.startsWith('data:') ||
        lowerUrl.startsWith('vbscript:') ||
        lowerUrl.startsWith('file:')) {
        return '#';
    }

    // Only allow http, https, and relative URLs
    if (!lowerUrl.startsWith('http://') &&
        !lowerUrl.startsWith('https://') &&
        !lowerUrl.startsWith('/') &&
        !lowerUrl.startsWith('#')) {
        // Assume relative URL if no protocol
        return url;
    }

    return url;
}

// =============================================================================
// UI Helpers
// =============================================================================

function setLoading(loading) {
    const addBtn = document.getElementById('add-btn');
    const btnText = addBtn.querySelector('.btn-text');
    const btnLoading = addBtn.querySelector('.btn-loading');
    const modelInput = document.getElementById('model-input');

    if (loading) {
        addBtn.disabled = true;
        modelInput.disabled = true;
        btnText.hidden = true;
        btnLoading.hidden = false;
    } else {
        addBtn.disabled = false;
        modelInput.disabled = false;
        btnText.hidden = false;
        btnLoading.hidden = true;
    }
}

function showProgress() {
    document.getElementById('progress-section').hidden = false;
}

function hideProgress() {
    document.getElementById('progress-section').hidden = true;
}

function setProgressTitle(title) {
    document.getElementById('progress-title').textContent = title;
}

function clearSteps() {
    document.getElementById('steps-list').innerHTML = '';
}

function addStep(name, success, message) {
    const stepsList = document.getElementById('steps-list');
    const icon = success ? '✓' : '✗';
    const statusClass = success ? 'step-success' : 'step-error';
    const stepName = formatStepName(name);

    const stepHtml = `
        <div class="step-item ${statusClass}">
            <span class="step-icon">${icon}</span>
            <span class="step-name">${stepName}</span>
            <span class="step-message">${escapeHtml(message)}</span>
        </div>
    `;

    stepsList.innerHTML += stepHtml;
}

function formatStepName(name) {
    const names = {
        'check': 'Check',
        'pull': 'Pull',
        'register': 'Register',
        'ollama': 'Ollama',
        'litellm': 'LiteLLM',
    };
    return names[name] || name;
}

function showResult(type, message) {
    const resultEl = document.getElementById('result-message');
    resultEl.className = 'result-message result-' + type;
    resultEl.textContent = message;
    resultEl.hidden = false;
}

function hideResult() {
    document.getElementById('result-message').hidden = true;
}

// =============================================================================
// Utility Functions
// =============================================================================

function formatSize(bytes) {
    if (!bytes) return 'Unknown';

    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIndex = 0;

    while (size >= 1024 && unitIndex < units.length - 1) {
        size /= 1024;
        unitIndex++;
    }

    return size.toFixed(1) + ' ' + units[unitIndex];
}

function formatDownloads(count) {
    if (!count) return '0';

    if (count >= 1000000) {
        return (count / 1000000).toFixed(1) + 'M';
    } else if (count >= 1000) {
        return (count / 1000).toFixed(1) + 'K';
    }
    return count.toString();
}

function formatDate(dateString) {
    if (!dateString) return 'Unknown';

    try {
        const date = new Date(dateString);
        const now = new Date();
        const diff = now - date;

        if (diff < 86400000) {
            const hours = Math.floor(diff / 3600000);
            if (hours < 1) {
                const minutes = Math.floor(diff / 60000);
                return minutes <= 1 ? 'just now' : minutes + ' minutes ago';
            }
            return hours === 1 ? '1 hour ago' : hours + ' hours ago';
        }

        if (diff < 604800000) {
            const days = Math.floor(diff / 86400000);
            return days === 1 ? '1 day ago' : days + ' days ago';
        }

        return date.toLocaleDateString();
    } catch (e) {
        return 'Unknown';
    }
}

function escapeHtml(text) {
    if (!text) return '';
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}
