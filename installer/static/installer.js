// Voiz Web Installer - Client-side Logic

let currentStep = 1;
const totalSteps = 7;
const config = {};

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    updateUI();
    loadDefaults();
});

// Load default values from form
function loadDefaults() {
    const form = document.getElementById('install-form');
    const inputs = form.querySelectorAll('input, select');

    inputs.forEach(input => {
        const name = input.name;
        if (!name) return;

        if (input.type === 'checkbox') {
            config[name] = input.checked ? 'true' : 'false';
        } else {
            config[name] = input.value || '';
        }
    });

    // Set hidden/computed values
    config['SECURITY_BASIC_AUTH_USERS_FILE'] = './auth/users.htpasswd';
    config['OLLAMA_HOST'] = '0.0.0.0:11434';
    config['OLLAMA_KEEP_ALIVE'] = '24h';
    config['OLLAMA_MAX_LOADED_MODELS'] = '3';
    config['WEBUI_OPENAI_API_BASE_URL'] = 'http://litellm:4000/v1';
    config['WEBUI_OPENAI_API_KEY'] = config['SECURITY_LITELLM_MASTER_KEY'];
    config['WEBUI_ENABLE_PERSISTENT_CONFIG'] = 'false';
    config['WEBUI_ENABLE_OLLAMA_API'] = 'false';
    config['WEBUI_ENABLE_OPENAI_API'] = 'true';
    config['SEARCH_ENGINE'] = 'searxng';
    config['SEARCH_SEARXNG_QUERY_URL'] = '"http://gluetun:8080/search?q=<query>&format=json"';
    config['SEARCH_SEARXNG_BASE_URL'] = '"http://gluetun:8080"';
    config['SEARCH_SEARXNG_SERVER_BASE_URL'] = '"http://localhost:8080"';
    config['ADVANCED_LITELLM_ROUTER_TIMEOUT'] = '120';
}

// Update form values in config
function updateConfig() {
    const form = document.getElementById('install-form');
    const inputs = form.querySelectorAll('input, select');

    inputs.forEach(input => {
        const name = input.name;
        if (!name) return;

        if (input.type === 'checkbox') {
            config[name] = input.checked ? 'true' : 'false';
        } else {
            config[name] = input.value || '';
        }
    });

    // Update derived values
    config['WEBUI_OPENAI_API_KEY'] = config['SECURITY_LITELLM_MASTER_KEY'];
}

// Generate secure random secret
function generateSecret(fieldName) {
    const array = new Uint8Array(32);
    crypto.getRandomValues(array);
    const secret = btoa(String.fromCharCode.apply(null, array));

    const input = document.querySelector(`input[name="${fieldName}"]`);
    if (input) {
        input.value = secret;
        updateConfig();
    }
}

// Generate all security secrets
function generateAllSecrets() {
    generateSecret('SECURITY_LITELLM_MASTER_KEY');
    generateSecret('SECURITY_LITELLM_SALT_KEY');
    generateSecret('SECURITY_SEARXNG_SECRET_KEY');
}

// Toggle VPN fields visibility
function toggleVPNFields() {
    const enabled = document.getElementById('enable-vpn').checked;
    const vpnFields = document.getElementById('vpn-fields');

    if (enabled) {
        vpnFields.classList.remove('hidden');
    } else {
        vpnFields.classList.add('hidden');
    }
}

// Toggle between OpenVPN and WireGuard fields
function toggleVPNProtocol() {
    const type = document.getElementById('vpn-type').value;
    const openvpnFields = document.getElementById('openvpn-fields');
    const wireguardFields = document.getElementById('wireguard-fields');

    if (type === 'openvpn') {
        openvpnFields.classList.remove('hidden');
        wireguardFields.classList.add('hidden');
    } else {
        openvpnFields.classList.add('hidden');
        wireguardFields.classList.remove('hidden');
    }
}

// Navigation
function nextStep() {
    if (currentStep < totalSteps) {
        updateConfig();
        currentStep++;
        updateUI();
    }
}

function previousStep() {
    if (currentStep > 1) {
        currentStep--;
        updateUI();
    }
}

function goToStep(step) {
    if (step >= 1 && step <= totalSteps) {
        updateConfig();
        currentStep = step;
        updateUI();
    }
}

// Update UI based on current step
function updateUI() {
    // Update steps
    document.querySelectorAll('.step').forEach(el => {
        el.classList.remove('active');
    });
    document.querySelector(`.step[data-step="${currentStep}"]`).classList.add('active');

    // Update progress
    const progress = (currentStep / totalSteps) * 100;
    document.getElementById('progress-bar').style.width = progress + '%';
    document.getElementById('current-step').textContent = currentStep;
    document.getElementById('progress-percent').textContent = Math.round(progress);

    // Update pills
    document.querySelectorAll('.step-pill').forEach(pill => {
        const step = parseInt(pill.dataset.step);
        pill.classList.remove('active', 'completed');

        if (step === currentStep) {
            pill.classList.add('active');
        } else if (step < currentStep) {
            pill.classList.add('completed');
        }
    });

    // Update buttons
    document.getElementById('btn-prev').disabled = currentStep === 1;

    if (currentStep === totalSteps) {
        document.getElementById('btn-next').style.display = 'none';
        document.getElementById('btn-install').style.display = 'block';
    } else {
        document.getElementById('btn-next').style.display = 'block';
        document.getElementById('btn-install').style.display = 'none';
    }
}

// Install function
async function install() {
    updateConfig();

    // Show modal
    document.getElementById('install-modal').classList.remove('hidden');
    document.getElementById('install-status').innerHTML = '<div class="text-gray-400">Preparing installation...</div>';

    try {
        // Send config to backend
        const response = await fetch('/api/install', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(config)
        });

        if (!response.ok) {
            throw new Error('Installation failed: ' + response.statusText);
        }

        // Stream progress
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let statusHTML = '';

        while (true) {
            const { value, done } = await reader.read();
            if (done) break;

            const chunk = decoder.decode(value);
            const lines = chunk.split('\n');

            lines.forEach(line => {
                if (line.trim()) {
                    try {
                        const data = JSON.parse(line);

                        if (data.status) {
                            statusHTML += `<div class="text-gray-400">${escapeHtml(data.status)}</div>`;
                            document.getElementById('install-status').innerHTML = statusHTML;
                        }

                        if (data.progress) {
                            document.getElementById('install-progress').style.width = data.progress + '%';
                        }

                        if (data.complete) {
                            document.getElementById('install-status').innerHTML = statusHTML;
                            document.getElementById('install-complete').classList.remove('hidden');
                            document.getElementById('final-host').textContent = config['DOMAIN_WEBUI_HOST'];

                            if (document.getElementById('enable-vpn')?.checked) {
                                document.getElementById('vpn-step').classList.remove('hidden');
                            }
                        }

                        if (data.error) {
                            throw new Error(data.error);
                        }
                    } catch (e) {
                        // Not JSON, just append as status
                        if (line.trim()) {
                            statusHTML += `<div class="text-gray-400">${escapeHtml(line)}</div>`;
                            document.getElementById('install-status').innerHTML = statusHTML;
                        }
                    }
                }
            });

            // Auto-scroll to bottom
            const statusDiv = document.getElementById('install-status');
            statusDiv.scrollTop = statusDiv.scrollHeight;
        }

    } catch (error) {
        console.error('Installation error:', error);
        document.getElementById('install-error').classList.remove('hidden');
        document.getElementById('error-message').textContent = error.message;
    }
}

// Close modal
function closeModal() {
    document.getElementById('install-modal').classList.add('hidden');
    document.getElementById('install-complete').classList.add('hidden');
    document.getElementById('install-error').classList.add('hidden');
}

// Utility: Escape HTML
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Add CSS for step pills
const style = document.createElement('style');
style.textContent = `
    .step-pill {
        padding: 0.5rem 1rem;
        border-radius: 0.375rem;
        font-size: 0.875rem;
        font-weight: 500;
        background-color: #374151;
        color: #9CA3AF;
        border: 2px solid transparent;
        transition: all 0.2s;
        cursor: pointer;
    }
    .step-pill:hover {
        background-color: #4B5563;
    }
    .step-pill.active {
        background-color: #2563EB;
        color: white;
        border-color: #3B82F6;
    }
    .step-pill.completed {
        background-color: #059669;
        color: white;
    }
    #install-status {
        max-height: 400px;
        overflow-y: auto;
    }
`;
document.head.appendChild(style);

// Add click handlers to step pills
document.querySelectorAll('.step-pill').forEach(pill => {
    pill.addEventListener('click', () => {
        const step = parseInt(pill.dataset.step);
        goToStep(step);
    });
});
