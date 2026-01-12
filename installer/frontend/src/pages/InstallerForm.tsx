import { useCallback, useEffect, useState } from 'react';
import { useForm, FormProvider } from 'react-hook-form';
import { Button, InfoBox, Modal, StatusLog, StepPills, ZoneLogo } from '../components';
import {
  useInstallation,
  useKeyboardNavigation,
} from '../hooks';
import {
  AdvancedStep,
  DomainStep,
  InterfaceStep,
  ModelsStep,
  SearchStep,
  SecurityStep,
  VPNStep,
} from '../steps';
import type { InstallerConfig } from '../types';
import { STEPS } from '../types';
import { StepSchemas, type StepSchemaKey } from '../validation/schemas';
import { loadConfig, saveConfig } from '../utils/crypto';

const DEFAULT_CONFIG: InstallerConfig = {
  // Domain
  DOMAIN_HOST_WEBUI: 'webui.localhost',

  // Security
  SECURITY_BASICAUTH_REALM: 'Zone AI Stack',
  SECURITY_LITELLM_MASTER_KEY: '',
  SECURITY_LITELLM_SALT_KEY: '',
  SECURITY_SEARXNG_SECRET_KEY: '',
  SECURITY_MANAGER_API_KEY: '',
  POSTGRES_PASSWORD: '',
  SECURITY_HTTP_REDIRECT: 'false',
  SECURITY_GENERATE_CERTIFICATE: 'false',

  // AI Provider
  AI_PROVIDER: 'self_hosted',
  AI_LITELLM_HOST: 'http://ollama:11434',
  AI_LITELLM_KEY: '',
  AI_OPENAI_API_KEY: '',
  AI_OPENAI_BASE_URL: '',
  AI_ANTHROPIC_API_KEY: '',
  AI_ANTHROPIC_BASE_URL: '',
  AI_BEDROCK_REGION: 'us-east-1',
  AI_BEDROCK_ACCESS_KEY: '',
  AI_BEDROCK_SECRET_KEY: '',
  AI_BEDROCK_USE_IAM_ROLE: 'false',
  AI_MODEL_FAST: 'llama3.1:8b',
  AI_MODEL_REASONING: 'deepseek-r1:32b',
  AI_MODEL_EMBEDDING: 'nomic-embed-text',

  // Interface
  WEBUI_AUTH: 'true',
  WEBUI_ENABLE_SIGNUP: 'false',
  WEBUI_DEFAULT_LOCALE: 'en-US',

  // Search
  SEARCH_ENABLE_WEB_SEARCH: 'true',
  SEARCH_RESULT_COUNT: '5',
  SEARCH_CONCURRENT_REQUESTS: '8',
  SEARCH_SEARXNG_INSTANCE_NAME: 'Zone Search',

  // VPN
  VPN_SERVICE_PROVIDER: 'surfshark',
  VPN_TYPE: 'openvpn',
  VPN_OPENVPN_USER: '',
  VPN_OPENVPN_PASSWORD: '',
  VPN_WIREGUARD_PRIVATE_KEY: '',
  VPN_WIREGUARD_ADDRESSES: '',
  VPN_SERVER_COUNTRIES: '',
  VPN_SERVER_CITIES: '',
  VPN_SERVER_REGIONS: '',

  // Monitoring
  MONITORING_ENABLED: 'false',
  MONITORING_GRAFANA_ADMIN_USER: 'admin',
  MONITORING_GRAFANA_ADMIN_PASSWORD: '',
  MONITORING_RETENTION_TIME: '15d',

  // Alerting
  ALERT_ENABLED: 'false',
  ALERT_EMAIL_RECIPIENTS: '',
  ALERT_SMTP_HOST: '',
  ALERT_SMTP_PORT: '587',
  ALERT_SMTP_USER: '',
  ALERT_SMTP_PASSWORD: '',
  ALERT_SMTP_FROM_ADDRESS: 'alerts@example.com',
  ALERT_SMTP_FROM_NAME: 'Zone Alerts',

  // Advanced
  ADVANCED_LITELLM_WORKERS: '4',
  ADVANCED_LITELLM_REQUEST_TIMEOUT: '600',
  ADVANCED_TZ: 'UTC',
  ADVANCED_ACME_EMAIL: 'admin@example.com',

  // Derived/computed values
  SECURITY_BASIC_AUTH_USERS_FILE: './auth/users.htpasswd',
  OLLAMA_HOST: '0.0.0.0:11434',
  OLLAMA_KEEP_ALIVE: '24h',
  OLLAMA_MAX_LOADED_MODELS: '3',
};

export default function InstallerForm() {
  const [currentStep, setCurrentStep] = useState(1);
  const [showModal, setShowModal] = useState(false);

  const methods = useForm<InstallerConfig>({
    defaultValues: DEFAULT_CONFIG,
    mode: 'onChange',
  });

  // Persistence
  useEffect(() => {
    loadConfig().then((stored) => {
      if (stored && Object.keys(stored).length > 0) {
        methods.reset({ ...DEFAULT_CONFIG, ...stored });
      }
    });
  }, [methods]);

  useEffect(() => {
    const subscription = methods.watch((value) => {
      if (value) saveConfig(value as InstallerConfig);
    });
    return () => subscription.unsubscribe();
  }, [methods]);

  const { isInstalling, progress, statusLines, isComplete, error, install, reset } =
    useInstallation();

  const totalSteps = STEPS.length;

  const handleNext = useCallback(() => {
    const stepId = STEPS[currentStep - 1].id as StepSchemaKey;
    const schema = StepSchemas[stepId];
    const result = schema.safeParse(methods.getValues());

    if (!result.success) {
      result.error.issues.forEach((issue) => {
        methods.setError(issue.path[0] as keyof InstallerConfig, { message: issue.message });
      });
      return;
    }

    methods.clearErrors();
    if (currentStep < totalSteps) {
      setCurrentStep((prev) => prev + 1);
    }
  }, [currentStep, totalSteps, methods]);

  const handlePrevious = useCallback(() => {
    if (currentStep > 1) {
      methods.clearErrors();
      setCurrentStep((prev) => prev - 1);
    }
  }, [currentStep, methods]);

  const handleStepClick = useCallback(
    (step: number) => {
      methods.clearErrors();
      setCurrentStep(step);
    },
    [methods]
  );

  const handleInstall = useCallback(() => {
    const stepId = STEPS[currentStep - 1].id as StepSchemaKey;
    const schema = StepSchemas[stepId];
    const result = schema.safeParse(methods.getValues());

    if (!result.success) {
      result.error.issues.forEach((issue) => {
        methods.setError(issue.path[0] as keyof InstallerConfig, { message: issue.message });
      });
      return;
    }

    setShowModal(true);
    install(methods.getValues());
  }, [currentStep, methods, install]);

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
        return <DomainStep />;
      case 2:
        return <SecurityStep />;
      case 3:
        return <ModelsStep />;
      case 4:
        return <InterfaceStep />;
      case 5:
        return <SearchStep />;
      case 6:
        return <VPNStep />;
      case 7:
        return <AdvancedStep />;
      default:
        return null;
    }
  };

  return (
    <div className="installer-layout">
      <aside className="installer-sidebar">
        <header className="sidebar-header">
          <ZoneLogo size="lg" />
          <p>Configuration</p>
        </header>
        <StepPills currentStep={currentStep} onStepClick={handleStepClick} />
      </aside>

      <main className="installer-main">
        <FormProvider {...methods}>
            <div className="card">
            {renderStep()}

            <div className="nav-buttons">
                <Button variant="secondary" onClick={handlePrevious} disabled={currentStep === 1}>
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
        </FormProvider>
      </main>

      <Modal
        isOpen={showModal}
        onClose={isComplete || error ? handleCloseModal : undefined}
        title={isInstalling ? 'Installing Zone...' : 'Installing Zone'}
      >
        <StatusLog lines={statusLines} />

        <div className="modal-progress">
          <div className="progress-bar-track">
            <div className="progress-bar-fill" style={{ width: `${progress}%` }} />
          </div>
        </div>

        {isComplete && (
          <InfoBox variant="success">
            <strong>Installation Complete</strong>
            <p style={{ marginTop: 'var(--space-sm)', fontSize: '0.875rem' }}>
              Run{' '}
              <code
                style={{
                  background: 'var(--bg-base)',
                  padding: '0.25rem 0.5rem',
                  borderRadius: '0.25rem',
                }}
              >
                make up
              </code>{' '}
              to start the stack.
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
