// Voiz Web Installer - Clean, Professional JavaScript

console.log('Voiz Installer JS loaded');

var currentStep = 1;
var totalSteps = 7;
var config = {};

// Function declarations (hoisted, available immediately)
function nextStep() {
    console.log('nextStep called');
    if (currentStep < totalSteps) {
        updateConfig();
        currentStep++;
        updateUI();
    }
}

function previousStep() {
    console.log('previousStep called');
    if (currentStep > 1) {
        currentStep--;
        updateUI();
    }
}

function goToStep(step) {
    console.log('goToStep called with step:', step);
    if (step >= 1 && step <= totalSteps) {
        updateConfig();
        currentStep = step;
        updateUI();
    }
}

function generateSecret(fieldName) {
    console.log('generateSecret called for:', fieldName);
    var array = new Uint8Array(32);
    crypto.getRandomValues(array);
    var secret = btoa(String.fromCharCode.apply(null, array));

    var input = document.querySelector('input[name="' + fieldName + '"]');
    if (input) {
        input.value = secret;
        updateConfig();
    }
}

function generateAllSecrets() {
    console.log('generateAllSecrets called');
    generateSecret('SECURITY_LITELLM_MASTER_KEY');
    generateSecret('SECURITY_LITELLM_SALT_KEY');
    generateSecret('SECURITY_SEARXNG_SECRET_KEY');
}

function toggleVPNFields() {
    var enabled = document.getElementById('enable-vpn').checked;
    var vpnFields = document.getElementById('vpn-fields');

    if (enabled) {
        vpnFields.classList.remove('hidden');
    } else {
        vpnFields.classList.add('hidden');
    }
}

function toggleVPNProtocol() {
    var type = document.getElementById('vpn-type').value;
    var openvpnFields = document.getElementById('openvpn-fields');
    var wireguardFields = document.getElementById('wireguard-fields');

    if (type === 'openvpn') {
        openvpnFields.classList.remove('hidden');
        wireguardFields.classList.add('hidden');
    } else {
        openvpnFields.classList.add('hidden');
        wireguardFields.classList.remove('hidden');
    }
}

function closeModal() {
    document.getElementById('install-modal').classList.remove('active');
    setTimeout(function() {
        document.getElementById('install-complete').classList.add('hidden');
        document.getElementById('install-error').classList.add('hidden');
    }, 200);
}

document.addEventListener('DOMContentLoaded', function() {
    console.log('DOM loaded, initializing...');
    updateUI();
    loadDefaults();
    attachEventListeners();
    console.log('Initialization complete');
});

function loadDefaults() {
    const form = document.getElementById('config-form');
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

    // Set computed values
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

function updateConfig() {
    const form = document.getElementById('config-form');
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

    config['WEBUI_OPENAI_API_KEY'] = config['SECURITY_LITELLM_MASTER_KEY'];
}

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

function generateAllSecrets() {
    generateSecret('SECURITY_LITELLM_MASTER_KEY');
    generateSecret('SECURITY_LITELLM_SALT_KEY');
    generateSecret('SECURITY_SEARXNG_SECRET_KEY');
}

function toggleVPNFields() {
    const enabled = document.getElementById('enable-vpn').checked;
    const vpnFields = document.getElementById('vpn-fields');

    if (enabled) {
        vpnFields.classList.remove('hidden');
    } else {
        vpnFields.classList.add('hidden');
    }
}

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


function updateUI() {
    console.log('updateUI called, currentStep:', currentStep);

    // Update steps
    const allSteps = document.querySelectorAll('.step');
    console.log('Found', allSteps.length, 'step elements');

    allSteps.forEach(el => {
        el.classList.remove('active');
    });

    const currentStepEl = document.querySelector(`.step[data-step="${currentStep}"]`);
    console.log('Current step element:', currentStepEl);

    if (currentStepEl) {
        currentStepEl.classList.add('active');
    } else {
        console.error('Could not find step element for step', currentStep);
    }

    // Update progress
    const progress = (currentStep / totalSteps) * 100;
    const progressBar = document.getElementById('progress-bar');
    if (progressBar) {
        progressBar.style.width = progress + '%';
    }

    const currentStepSpan = document.getElementById('current-step');
    if (currentStepSpan) {
        currentStepSpan.textContent = currentStep;
    }

    const progressPercent = document.getElementById('progress-percent');
    if (progressPercent) {
        progressPercent.textContent = Math.round(progress);
    }

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
    const btnPrev = document.getElementById('btn-prev');
    const btnNext = document.getElementById('btn-next');
    const btnInstall = document.getElementById('btn-install');

    if (btnPrev) btnPrev.disabled = currentStep === 1;

    if (currentStep === totalSteps) {
        if (btnNext) btnNext.style.display = 'none';
        if (btnInstall) btnInstall.style.display = 'block';
    } else {
        if (btnNext) btnNext.style.display = 'block';
        if (btnInstall) btnInstall.style.display = 'none';
    }

    console.log('updateUI complete');
}

function install() {
    console.log('install called');
    updateConfig();

    var modal = document.getElementById('install-modal');
    modal.classList.add('active');

    document.getElementById('install-status').innerHTML =
        '<div class="status-line">Preparing installation...</div>';

    fetch('/api/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(config)
    })
    .then(function(response) {
        if (!response.ok) {
            throw new Error('Installation failed: ' + response.statusText);
        }
        return response.body.getReader();
    })
    .then(function(reader) {
        var decoder = new TextDecoder();
        var statusHTML = '';

        function readChunk() {
            return reader.read().then(function(result) {
                if (result.done) return;

                var chunk = decoder.decode(result.value);
                var lines = chunk.split('\n');

                lines.forEach(function(line) {
                    if (line.trim()) {
                        try {
                            var data = JSON.parse(line);

                            if (data.status) {
                                var className = data.status.includes('✓') ? 'success' : '';
                                statusHTML += '<div class="status-line ' + className + '">' + escapeHtml(data.status) + '</div>';
                                document.getElementById('install-status').innerHTML = statusHTML;
                            }

                            if (data.progress) {
                                document.getElementById('install-progress').style.width = data.progress + '%';
                            }

                            if (data.complete) {
                                setTimeout(function() {
                                    document.getElementById('install-complete').classList.remove('hidden');
                                }, 300);
                            }

                            if (data.error) {
                                throw new Error(data.error);
                            }
                        } catch (e) {
                            if (line.trim() && !line.includes('{')) {
                                statusHTML += '<div class="status-line">' + escapeHtml(line) + '</div>';
                                document.getElementById('install-status').innerHTML = statusHTML;
                            }
                        }
                    }
                });

                var statusDiv = document.getElementById('install-status');
                statusDiv.scrollTop = statusDiv.scrollHeight;

                return readChunk();
            });
        }

        return readChunk();
    })
    .catch(function(error) {
        console.error('Installation error:', error);
        document.getElementById('install-error').classList.remove('hidden');
        document.getElementById('error-message').textContent = error.message;
    });
}

function closeModal() {
    document.getElementById('install-modal').classList.remove('active');
    setTimeout(() => {
        document.getElementById('install-complete').classList.add('hidden');
        document.getElementById('install-error').classList.add('hidden');
    }, 200);
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function attachEventListeners() {
    // Step pill click handlers
    document.querySelectorAll('.step-pill').forEach(pill => {
        pill.addEventListener('click', () => {
            const step = parseInt(pill.dataset.step);
            goToStep(step);
        });
    });

    // Keyboard navigation
    document.addEventListener('keydown', (e) => {
        if (e.key === 'ArrowRight' && currentStep < totalSteps) {
            nextStep();
        } else if (e.key === 'ArrowLeft' && currentStep > 1) {
            previousStep();
        }
    });
}
