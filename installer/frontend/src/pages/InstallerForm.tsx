import React, { useState, useCallback } from 'react';
import {
  Button,
  Modal,
  StatusLog,
  ProgressBar,
  StepPills,
  InfoBox,
} from '../components';
import {
  DomainStep,
  SecurityStep,
  ModelsStep,
  InterfaceStep,
  SearchStep,
  VPNStep,
  AdvancedStep,
} from '../steps';
import { useInstallation, useKeyboardNavigation } from '../hooks';
import type { InstallerConfig } from '../types';
import { STEPS } from '../types';

const DEFAULT_CONFIG: InstallerConfig = {
  // Domain
  WEBUI_HOSTNAME: 'webui.localhost',

  // Security
  SECURITY_AUTH_REALM: 'Zone AI Stack',
  LITELLM_MASTER_KEY: 'dev-insecure-key-change-for-production',
  LITELLM_SALT_KEY: 'dev-insecure-salt-change-for-production',
  SEARXNG_SECRET_KEY: 'dev-insecure-key-change-for-production',

  // Models
  OLLAMA_FAST_MODEL: 'llama3.1:8b',
  OLLAMA_REASONING_MODEL: 'deepseek-r1:32b',
  OLLAMA_EMBEDDING_MODEL: 'nomic-embed-text',

  // Interface
  WEBUI_AUTH: 'false',
  WEBUI_ENABLE_SIGNUP: 'false',
  WEBUI_DEFAULT_LOCALE: 'en-US',

  // Search
  ENABLE_RAG_WEB_SEARCH: 'true',
  RAG_WEB_SEARCH_RESULT_COUNT: '5',
  RAG_WEB_SEARCH_CONCURRENT_REQUESTS: '8',
  SEARXNG_INSTANCE_NAME: 'Zone Search',

  // VPN
  ENABLE_VPN: 'false',
  VPN_PROVIDER: 'surfshark',
  VPN_PROTOCOL: 'openvpn',
  OPENVPN_USER: '',
  OPENVPN_PASS: '',
  WIREGUARD_PRIVATE_KEY: '',
  WIREGUARD_ADDRESS: '',

  // Advanced - Monitoring
  ENABLE_MONITORING: 'false',
  GF_SECURITY_ADMIN_USER: 'admin',
  GF_SECURITY_ADMIN_PASSWORD: '',
  METRICS_RETENTION: '15d',

  // Advanced - Performance
  WORKERS: '4',
  REQUEST_TIMEOUT: '600',
  TZ: 'UTC',
  ACME_EMAIL: 'admin@example.com',

  // Derived/computed values
  SECURITY_BASIC_AUTH_USERS_FILE: './auth/users.htpasswd',
  OLLAMA_HOST: '0.0.0.0:11434',
  OLLAMA_KEEP_ALIVE: '24h',
  OLLAMA_MAX_LOADED_MODELS: '3',
  WEBUI_OPENAI_API_BASE_URL: 'http://litellm:4000/v1',
  WEBUI_OPENAI_API_KEY: '',
};

export default function InstallerForm() {
  const [currentStep, setCurrentStep] = useState(1);
  const [config, setConfig] = useState<InstallerConfig>(DEFAULT_CONFIG);
  const [showModal, setShowModal] = useState(false);

  const {
    isInstalling,
    progress,
    statusLines,
    isComplete,
    error,
    install,
    reset,
  } = useInstallation();

  const totalSteps = STEPS.length;

  const handleChange = useCallback((key: keyof InstallerConfig, value: string) => {
    setConfig(prev => {
      const updated = { ...prev, [key]: value };
      // Sync WEBUI_OPENAI_API_KEY with LITELLM_MASTER_KEY
      if (key === 'LITELLM_MASTER_KEY') {
        updated.WEBUI_OPENAI_API_KEY = value;
      }
      return updated;
    });
  }, []);

  const handleNext = useCallback(() => {
    if (currentStep < totalSteps) {
      setCurrentStep(prev => prev + 1);
    }
  }, [currentStep, totalSteps]);

  const handlePrevious = useCallback(() => {
    if (currentStep > 1) {
      setCurrentStep(prev => prev - 1);
    }
  }, [currentStep]);

  const handleStepClick = useCallback((step: number) => {
    setCurrentStep(step);
  }, []);

  const handleInstall = useCallback(() => {
    setShowModal(true);
    install(config);
  }, [config, install]);

  const handleCloseModal = useCallback(() => {
    setShowModal(false);
    reset();
  }, [reset]);

  useKeyboardNavigation({
    currentStep,
    totalSteps,
    onNext: handleNext,
    onPrevious: handlePrevious,
    enabled: !showModal,
  });

  const renderStep = () => {
    switch (currentStep) {
      case 1:
        return <DomainStep config={config} onChange={handleChange} />;
      case 2:
        return <SecurityStep config={config} onChange={handleChange} />;
      case 3:
        return <ModelsStep config={config} onChange={handleChange} />;
      case 4:
        return <InterfaceStep config={config} onChange={handleChange} />;
      case 5:
        return <SearchStep config={config} onChange={handleChange} />;
      case 6:
        return <VPNStep config={config} onChange={handleChange} />;
      case 7:
        return <AdvancedStep config={config} onChange={handleChange} />;
      default:
        return null;
    }
  };

  return (
    <div className="container">
      <header className="header">
        <h1>Zone Configuration</h1>
        <p>Set up your self-hosted AI stack</p>
      </header>

      <ProgressBar currentStep={currentStep} />
      <StepPills currentStep={currentStep} onStepClick={handleStepClick} />

      <div className="card">
        {renderStep()}

        <div className="nav-buttons">
          <Button
            variant="secondary"
            onClick={handlePrevious}
            disabled={currentStep === 1}
          >
            Previous
          </Button>

          {currentStep < totalSteps ? (
            <Button variant="primary" onClick={handleNext}>
              Next
            </Button>
          ) : (
            <Button variant="primary" onClick={handleInstall}>
              Install
            </Button>
          )}
        </div>
      </div>

      <Modal
        isOpen={showModal}
        onClose={isComplete || error ? handleCloseModal : undefined}
        title={isInstalling ? "Installing Zone..." : "Installing Zone"}
      >
        <StatusLog lines={statusLines} />

        <div className="modal-progress">
          <div className="progress-bar-track">
            <div
              className="progress-bar-fill"
              style={{ width: `${progress}%` }}
            />
          </div>
        </div>

        {isComplete && (
          <InfoBox variant="success">
            <strong>Installation Complete</strong>
            <p style={{ marginTop: 'var(--space-sm)', fontSize: '0.875rem' }}>
              Run <code style={{ background: 'var(--bg-base)', padding: '0.25rem 0.5rem', borderRadius: '0.25rem' }}>make up</code> to start the stack.
            </p>
          </InfoBox>
        )}

        {error && (
          <InfoBox variant="warning">
            <strong>Installation Failed</strong>
            <p className="font-mono" style={{ marginTop: 'var(--space-sm)', fontSize: '0.875rem' }}>
              {error}
            </p>
          </InfoBox>
        )}

        {(isComplete || error) && (
          <div className="modal-buttons">
            <Button variant="primary" onClick={handleCloseModal} className="w-full">
              Close
            </Button>
          </div>
        )}
      </Modal>
    </div>
  );
}
