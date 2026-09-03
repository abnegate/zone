import { useCallback, useEffect, useState } from 'react';
import { FormProvider, useForm } from 'react-hook-form';
import {
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  InfoBox,
  Modal,
  ProgressBar,
  Separator,
  StatusLog,
  StepPills,
  ZoneLogo,
} from '../components';
import { InstallSummary } from '../components/InstallSummary';
import { useInstallation } from '../hooks/useInstallation';
import { useKeyboardNavigation } from '../hooks/useKeyboardNavigation';
import { AdvancedStep } from '../steps/AdvancedStep';
import { DomainStep } from '../steps/DomainStep';
import { InterfaceStep } from '../steps/InterfaceStep';
import { ModelsStep } from '../steps/ModelsStep';
import { SearchStep } from '../steps/SearchStep';
import { SecurityStep } from '../steps/SecurityStep';
import { VPNStep } from '../steps/VPNStep';
import type { InstallerConfig } from '../types';
import { STEPS } from '../types';
import { loadConfig, saveConfig } from '../utils/crypto';
import type { StepSchemaKey } from '../validation/schemas';

const loadStepSchema = async (stepId: StepSchemaKey) => {
  const module = await import('../validation/schemas');
  return module.StepSchemas[stepId];
};

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
  const [showCompletionPage, setShowCompletionPage] = useState(false);
  const [completionSnapshot, setCompletionSnapshot] = useState<{
    completedAt: Date;
    summaryRows: Array<{ label: string; value: string }>;
    webUiHost: string;
  } | null>(null);
  const [completedAt, setCompletedAt] = useState<Date | null>(null);
  const [summaryRows, setSummaryRows] = useState<Array<{ label: string; value: string }>>([]);

  const methods = useForm<InstallerConfig>({
    defaultValues: DEFAULT_CONFIG,
    mode: 'onChange',
  });

  const { isInstalling, progress, statusLines, isComplete, error, install, reset } =
    useInstallation();

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

  useEffect(() => {
    if (!isComplete) {
      setCompletedAt(null);
      setSummaryRows([]);
      return;
    }

    const summary = [
      { label: 'Web UI Host', value: methods.getValues('DOMAIN_HOST_WEBUI') || '—' },
      { label: 'AI Provider', value: methods.getValues('AI_PROVIDER') || '—' },
      {
        label: 'Web UI Auth',
        value: methods.getValues('WEBUI_AUTH') === 'true' ? 'Enabled' : 'Disabled',
      },
      {
        label: 'Web Search',
        value: methods.getValues('SEARCH_ENABLE_WEB_SEARCH') === 'true' ? 'Enabled' : 'Disabled',
      },
      { label: 'VPN Provider', value: methods.getValues('VPN_SERVICE_PROVIDER') || '—' },
    ];
    const completionTime = completedAt ?? new Date();
    const webUiHost = methods.getValues('DOMAIN_HOST_WEBUI') || '';

    setCompletedAt(completionTime);
    setSummaryRows(summary);
    setCompletionSnapshot(
      (prev) => prev ?? { completedAt: completionTime, summaryRows: summary, webUiHost }
    );
  }, [completedAt, isComplete, methods]);

  const totalSteps = STEPS.length;
  const stepMeta = STEPS[currentStep - 1] ?? STEPS[0];

  const applyResolverErrors = useCallback(
    (errors: Record<string, { message?: unknown } | undefined>) => {
      for (const [field, error] of Object.entries(errors)) {
        const message = typeof error?.message === 'string' ? error.message : 'Invalid value';
        methods.setError(field as keyof InstallerConfig, { message });
      }
    },
    [methods]
  );

  const validateStep = useCallback(
    async (stepId: StepSchemaKey) => {
      const schema = await loadStepSchema(stepId);
      const { zodResolver } = await import('@hookform/resolvers/zod');
      const resolver = zodResolver(schema);
      const result = await resolver(methods.getValues(), undefined, {
        criteriaMode: 'all',
        shouldUseNativeValidation: false,
        fields: {},
        names: [],
      });

      if (Object.keys(result.errors).length > 0) {
        applyResolverErrors(result.errors as Record<string, { message?: unknown } | undefined>);
        return false;
      }

      return true;
    },
    [applyResolverErrors, methods]
  );

  const validateCurrentStep = useCallback(async () => {
    methods.clearErrors();
    const stepId = STEPS[currentStep - 1].id as StepSchemaKey;
    return validateStep(stepId);
  }, [currentStep, methods, validateStep]);

  const validateAllSteps = useCallback(async () => {
    methods.clearErrors();
    const stepIds = STEPS.map((step) => step.id as StepSchemaKey);
    let firstInvalidIndex: number | null = null;

    for (const [index, stepId] of stepIds.entries()) {
      const isValid = await validateStep(stepId);
      if (!isValid && firstInvalidIndex === null) {
        firstInvalidIndex = index;
      }
    }

    if (firstInvalidIndex !== null) {
      setCurrentStep(firstInvalidIndex + 1);
      return false;
    }

    return true;
  }, [methods, setCurrentStep, validateStep]);

  const handleNext = useCallback(async () => {
    const isValid = await validateCurrentStep();
    if (!isValid) {
      return;
    }

    if (currentStep < totalSteps) {
      setCurrentStep((prev) => prev + 1);
    }
  }, [currentStep, totalSteps, validateCurrentStep]);

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

  const handleInstall = useCallback(async () => {
    const isValid = await validateAllSteps();
    if (!isValid) {
      return;
    }

    setShowCompletionPage(false);
    setCompletionSnapshot(null);
    setShowModal(true);
    install(methods.getValues());
  }, [install, methods, validateAllSteps]);

  const handleCloseModal = useCallback(() => {
    setShowModal(false);
    if (isComplete) {
      setShowCompletionPage(true);
      setCompletionSnapshot((prev) => {
        if (prev) {
          return prev;
        }
        const summary = [
          { label: 'Web UI Host', value: methods.getValues('DOMAIN_HOST_WEBUI') || '—' },
          { label: 'AI Provider', value: methods.getValues('AI_PROVIDER') || '—' },
          {
            label: 'Web UI Auth',
            value: methods.getValues('WEBUI_AUTH') === 'true' ? 'Enabled' : 'Disabled',
          },
          {
            label: 'Web Search',
            value:
              methods.getValues('SEARCH_ENABLE_WEB_SEARCH') === 'true' ? 'Enabled' : 'Disabled',
          },
          { label: 'VPN Provider', value: methods.getValues('VPN_SERVICE_PROVIDER') || '—' },
        ];
        return {
          completedAt: completedAt ?? new Date(),
          summaryRows: summary,
          webUiHost: methods.getValues('DOMAIN_HOST_WEBUI') || '',
        };
      });
    }
    reset();
    if (typeof window !== 'undefined') {
      window.close();
    }
  }, [completedAt, isComplete, methods, reset]);

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

  if (showCompletionPage && completionSnapshot) {
    const webUiHost = completionSnapshot.webUiHost.trim();
    const webUiUrl =
      webUiHost.length === 0
        ? ''
        : webUiHost.startsWith('http://') || webUiHost.startsWith('https://')
          ? webUiHost
          : `http://${webUiHost}`;

    return (
      <div className="min-h-screen bg-muted/30 text-foreground">
        <div className="mx-auto w-full max-w-3xl px-4 py-8">
          <Card>
            <CardHeader className="space-y-2">
              <ZoneLogo size="md" />
              <CardTitle>Installation complete</CardTitle>
              <CardDescription>
                Zone is up and running. If this tab did not close automatically, you can close it
                now.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <InfoBox variant="success">
                <AlertTitle>Docker Compose stack started</AlertTitle>
                <AlertDescription className="flex flex-wrap items-center gap-2">
                  <span>Use</span>
                  <code className="rounded-md bg-muted px-2 py-1 text-xs">
                    docker compose logs -f
                  </code>
                  <span>to monitor.</span>
                </AlertDescription>
              </InfoBox>

              {webUiUrl && (
                <InfoBox variant="info">
                  <AlertTitle>Open Web UI</AlertTitle>
                  <AlertDescription>
                    <a className="underline" href={webUiUrl} target="_blank" rel="noreferrer">
                      {webUiUrl}
                    </a>
                  </AlertDescription>
                </InfoBox>
              )}

              {completionSnapshot.summaryRows.length > 0 && (
                <InstallSummary
                  rows={completionSnapshot.summaryRows}
                  completedAt={completionSnapshot.completedAt}
                />
              )}
            </CardContent>
            <CardFooter className="justify-between gap-3">
              <Button variant="secondary" onClick={() => setShowCompletionPage(false)}>
                Back to configuration
              </Button>
              <Button onClick={() => window.close()}>Close tab</Button>
            </CardFooter>
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-muted/30 text-foreground">
      <div className="mx-auto w-full max-w-4xl px-4 py-6">
        <div className="grid items-start gap-6 lg:grid-cols-[220px_minmax(0,1fr)]">
          <aside className="lg:sticky lg:top-6" data-testid="installer-sidebar">
            <Card>
              <CardHeader className="pb-3">
                <ZoneLogo size="md" />
                <p className="font-display text-sm font-semibold text-muted-foreground">
                  Configuration
                </p>
              </CardHeader>
              <Separator />
              <CardContent className="pt-4">
                <StepPills currentStep={currentStep} onStepClick={handleStepClick} />
              </CardContent>
            </Card>
          </aside>

          <main className="min-w-0" data-testid="installer-main">
            <FormProvider {...methods}>
              <Card className="w-full" data-testid="installer-card">
                <CardHeader className="border-b">
                  <CardTitle>{stepMeta.title}</CardTitle>
                  <CardDescription>{stepMeta.description}</CardDescription>
                  <div className="pt-4">
                    <ProgressBar
                      value={currentStep}
                      max={totalSteps}
                      showPercentage={false}
                      label={`Step ${currentStep} of ${totalSteps}`}
                    />
                  </div>
                </CardHeader>
                <CardContent className="space-y-6 pt-6">{renderStep()}</CardContent>

                <CardFooter className="justify-between border-t px-6 py-4">
                  <Button variant="secondary" onClick={handlePrevious} disabled={currentStep === 1}>
                    Previous
                  </Button>

                  {currentStep < totalSteps ? (
                    <Button onClick={handleNext}>Next</Button>
                  ) : (
                    <Button onClick={handleInstall}>Install</Button>
                  )}
                </CardFooter>
              </Card>
            </FormProvider>
          </main>
        </div>
      </div>

      <Modal
        isOpen={showModal}
        onClose={isComplete || error ? handleCloseModal : undefined}
        title={
          isComplete
            ? 'Installation complete'
            : isInstalling
              ? 'Installing Zone...'
              : 'Installing Zone'
        }
        size="xl"
        className="max-h-[90vh] w-[90vw] max-w-[900px] overflow-y-auto"
      >
        <div className="space-y-4">
          <StatusLog lines={statusLines} />
          <ProgressBar value={progress} showPercentage={false} />

          {isComplete && (
            <InfoBox variant="success">
              <AlertTitle>Installation Complete</AlertTitle>
              <AlertDescription className="flex flex-wrap items-center gap-2">
                <span>Docker Compose stack started.</span>
                <span>Use</span>
                <code className="rounded-md bg-muted px-2 py-1 text-xs">
                  docker compose logs -f
                </code>
                <span>to monitor.</span>
                <span>The installer will shut down automatically.</span>
              </AlertDescription>
            </InfoBox>
          )}

          {isComplete && completedAt && summaryRows.length > 0 && (
            <InstallSummary rows={summaryRows} completedAt={completedAt} />
          )}

          {error && (
            <InfoBox variant="warning">
              <AlertTitle>Installation Failed</AlertTitle>
              <AlertDescription className="font-mono text-xs">{error}</AlertDescription>
            </InfoBox>
          )}

          {(isComplete || error) && (
            <Button onClick={handleCloseModal} className="w-full">
              Close
            </Button>
          )}
        </div>
      </Modal>
    </div>
  );
}
