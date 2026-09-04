import { Button, Tabs, TabsContent, TabsList, TabsTrigger } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useAuth } from '../../../auth';
import {
  AuditLogsSection,
  BillingSection,
  InvitationsSection,
  OrgMembersSection,
} from '../components';
import type { AiProvider, AiSettings, UpdateAiSettingsRequest, Workspace } from '../types';
import '../../workspace/pages/WorkspaceSettingsPage.css';

type TabType = 'ai' | 'members' | 'invitations' | 'billing' | 'audit';

const providerOptions: { value: AiProvider; label: string }[] = [
  { value: 'self_hosted', label: 'Self-Hosted (Ollama via LiteLLM)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'bedrock', label: 'AWS Bedrock' },
];

const modelOptions = {
  self_hosted: {
    fast: ['llama3.2:3b', 'llama3.1:8b', 'qwen2.5:7b', 'mistral:7b'],
    reasoning: ['deepseek-r1:7b', 'deepseek-r1:14b', 'deepseek-r1:32b', 'llama3.1:70b'],
    embedding: ['nomic-embed-text', 'mxbai-embed-large'],
  },
  openai: {
    fast: ['gpt-4o-mini', 'gpt-4o', 'gpt-4-turbo'],
    reasoning: ['gpt-4o', 'o1', 'o1-mini'],
    embedding: ['text-embedding-3-small', 'text-embedding-3-large', 'text-embedding-ada-002'],
  },
  anthropic: {
    fast: ['claude-3-haiku-20240307', 'claude-sonnet-4-20250514'],
    reasoning: ['claude-sonnet-4-20250514', 'claude-opus-4-20250514'],
    embedding: [] as string[],
  },
  bedrock: {
    fast: [
      'anthropic.claude-3-haiku-20240307-v1:0',
      'amazon.nova-lite-v1:0',
      'amazon.nova-micro-v1:0',
    ],
    reasoning: [
      'anthropic.claude-3-5-sonnet-20241022-v2:0',
      'amazon.nova-pro-v1:0',
      'anthropic.claude-3-opus-20240229-v1:0',
    ],
    embedding: [
      'amazon.titan-embed-text-v2:0',
      'amazon.titan-embed-text-v1',
      'cohere.embed-english-v3',
    ],
  },
};

const IMAGE_MODEL_OPTIONS = ['flux1-schnell-fp8.safetensors'];

const awsRegions = [
  'us-east-1',
  'us-west-2',
  'eu-west-1',
  'eu-central-1',
  'ap-northeast-1',
  'ap-southeast-1',
  'ap-southeast-2',
];

export default function OrgSettingsPage() {
  const { isAuthenticated } = useAuth();
  const { currentOrganization } = useWorkspace();

  const [activeTab, setActiveTab] = useState<TabType>('ai');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);

  // Form state
  const [provider, setProvider] = useState<AiProvider>('self_hosted');
  const [litellmHost, setLitellmHost] = useState('');
  const [litellmKey, setLitellmKey] = useState('');
  const [openaiApiKey, setOpenaiApiKey] = useState('');
  const [openaiBaseUrl, setOpenaiBaseUrl] = useState('');
  const [anthropicApiKey, setAnthropicApiKey] = useState('');
  const [anthropicBaseUrl, setAnthropicBaseUrl] = useState('');
  const [bedrockRegion, setBedrockRegion] = useState('us-east-1');
  const [bedrockAccessKey, setBedrockAccessKey] = useState('');
  const [bedrockSecretKey, setBedrockSecretKey] = useState('');
  const [bedrockUseIamRole, setBedrockUseIamRole] = useState(false);
  const [modelFast, setModelFast] = useState('');
  const [modelReasoning, setModelReasoning] = useState('');
  const [modelEmbedding, setModelEmbedding] = useState('');
  const [modelImage, setModelImage] = useState('');

  // Track which credentials are set on server
  const [hasLitellmKey, setHasLitellmKey] = useState(false);
  const [hasOpenaiKey, setHasOpenaiKey] = useState(false);
  const [hasAnthropicKey, setHasAnthropicKey] = useState(false);
  const [hasBedrockCreds, setHasBedrockCreds] = useState(false);

  const loadSettings = useCallback(async () => {
    if (!isAuthenticated || !currentOrganization) return;
    setLoading(true);
    setError(null);
    try {
      const [settings, workspacesData] = await Promise.all([
        client.getOrgAiSettings(currentOrganization.id),
        client.getWorkspaces(currentOrganization.id),
      ]);
      applySettingsToForm(settings);
      setWorkspaces(workspacesData);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load settings');
    } finally {
      setLoading(false);
    }
  }, [isAuthenticated, currentOrganization]);

  const applySettingsToForm = (settings: AiSettings) => {
    setProvider(settings.provider);
    setLitellmHost(settings.litellm_host || '');
    setOpenaiBaseUrl(settings.openai_base_url || '');
    setAnthropicBaseUrl(settings.anthropic_base_url || '');
    setBedrockRegion(settings.bedrock_region || 'us-east-1');
    setBedrockUseIamRole(settings.bedrock_use_iam_role);
    setModelFast(settings.model_fast || '');
    setModelReasoning(settings.model_reasoning || '');
    setModelEmbedding(settings.model_embedding || '');
    setModelImage(settings.model_image || '');
    setHasLitellmKey(settings.has_litellm_key);
    setHasOpenaiKey(settings.has_openai_api_key);
    setHasAnthropicKey(settings.has_anthropic_api_key);
    setHasBedrockCreds(settings.has_bedrock_credentials);
    // Clear password fields on load
    setLitellmKey('');
    setOpenaiApiKey('');
    setAnthropicApiKey('');
    setBedrockAccessKey('');
    setBedrockSecretKey('');
  };

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !currentOrganization) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      const request: UpdateAiSettingsRequest = {
        provider,
        model_fast: modelFast || undefined,
        model_reasoning: modelReasoning || undefined,
        model_embedding: modelEmbedding || undefined,
        model_image: modelImage || undefined,
      };

      // Only include credentials if they were entered
      if (provider === 'self_hosted') {
        request.litellm_host = litellmHost || undefined;
        if (litellmKey) request.litellm_key = litellmKey;
      } else if (provider === 'openai') {
        request.openai_base_url = openaiBaseUrl || undefined;
        if (openaiApiKey) request.openai_api_key = openaiApiKey;
      } else if (provider === 'anthropic') {
        request.anthropic_base_url = anthropicBaseUrl || undefined;
        if (anthropicApiKey) request.anthropic_api_key = anthropicApiKey;
      } else if (provider === 'bedrock') {
        request.bedrock_region = bedrockRegion || undefined;
        request.bedrock_use_iam_role = bedrockUseIamRole;
        if (!bedrockUseIamRole) {
          if (bedrockAccessKey) request.bedrock_access_key = bedrockAccessKey;
          if (bedrockSecretKey) request.bedrock_secret_key = bedrockSecretKey;
        }
      }

      const settings = await client.updateOrgAiSettings(currentOrganization.id, request);
      applySettingsToForm(settings);
      setSuccess('Settings saved successfully');
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save settings');
    } finally {
      setSaving(false);
    }
  };

  const handleReset = async () => {
    if (!isAuthenticated || !currentOrganization) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      const settings = await client.resetOrgAiSettings(currentOrganization.id);
      applySettingsToForm(settings);
      setSuccess('Settings reset to defaults');
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to reset settings');
    } finally {
      setSaving(false);
    }
  };

  const currentModels = modelOptions[provider];

  if (!currentOrganization) {
    return (
      <div className="page page--workspace settings-page">
        <header className="settings-page-header">
          <h1 className="page-title">Organization Settings</h1>
        </header>
        <div className="settings-page-body">
          <div className="loading-state">Please select an organization</div>
        </div>
      </div>
    );
  }

  if (loading && activeTab === 'ai') {
    return (
      <div className="page page--workspace settings-page">
        <header className="settings-page-header">
          <h1 className="page-title">Organization Settings</h1>
        </header>
        <div className="settings-page-body">
          <div className="loading-state">Loading settings...</div>
        </div>
      </div>
    );
  }

  return (
    <div className="page page--workspace settings-page">
      <header className="settings-page-header">
        <h1 className="page-title">Organization Settings</h1>
      </header>
      <div className="settings-page-body">
        {error && <div className="alert alert-error">{error}</div>}
        {success && <div className="alert alert-success">{success}</div>}

        <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as TabType)}>
          <TabsList aria-label="Organization settings">
            <TabsTrigger value="ai">AI Settings</TabsTrigger>
            <TabsTrigger value="members">Members</TabsTrigger>
            <TabsTrigger value="invitations">Invitations</TabsTrigger>
            <TabsTrigger value="billing">Billing</TabsTrigger>
            <TabsTrigger value="audit">Audit Logs</TabsTrigger>
          </TabsList>

          <TabsContent value="members">
            <OrgMembersSection orgId={currentOrganization.id} />
          </TabsContent>
          <TabsContent value="invitations">
            <InvitationsSection orgId={currentOrganization.id} workspaces={workspaces} />
          </TabsContent>
          <TabsContent value="billing">
            <BillingSection orgId={currentOrganization.id} />
          </TabsContent>
          <TabsContent value="audit">
            <AuditLogsSection orgId={currentOrganization.id} />
          </TabsContent>
          <TabsContent value="ai">
            <form onSubmit={handleSave} className="settings-form">
              <section className="settings-section">
                <h2 className="section-title">AI Provider Configuration</h2>
                <p className="section-description">
                  Configure the default AI provider and models for this organization. These settings
                  can be overridden at the workspace level.
                </p>

                <div className="settings-card">
                  <h3 className="card-title">Provider Selection</h3>
                  <div className="form-group">
                    <label htmlFor="provider">AI Provider</label>
                    <select
                      id="provider"
                      value={provider}
                      onChange={(e) => setProvider(e.target.value as AiProvider)}
                      className="form-select"
                    >
                      {providerOptions.map((opt) => (
                        <option key={opt.value} value={opt.value}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>

                {/* Self-hosted settings */}
                {provider === 'self_hosted' && (
                  <div className="settings-card">
                    <h3 className="card-title">LiteLLM Configuration</h3>
                    <div className="form-group">
                      <label htmlFor="litellm-host">LiteLLM Host</label>
                      <input
                        type="text"
                        id="litellm-host"
                        value={litellmHost}
                        onChange={(e) => setLitellmHost(e.target.value)}
                        placeholder="http://ollama:11434"
                        className="form-input"
                      />
                    </div>
                    <div className="form-group">
                      <label htmlFor="litellm-key">
                        API Key {hasLitellmKey && <span className="credential-set">(set)</span>}
                      </label>
                      <input
                        type="password"
                        id="litellm-key"
                        value={litellmKey}
                        onChange={(e) => setLitellmKey(e.target.value)}
                        placeholder={hasLitellmKey ? '********' : 'Optional'}
                        className="form-input"
                      />
                    </div>
                  </div>
                )}

                {/* OpenAI settings */}
                {provider === 'openai' && (
                  <div className="settings-card">
                    <h3 className="card-title">OpenAI Configuration</h3>
                    <div className="form-group">
                      <label htmlFor="openai-key">
                        API Key {hasOpenaiKey && <span className="credential-set">(set)</span>}
                      </label>
                      <input
                        type="password"
                        id="openai-key"
                        value={openaiApiKey}
                        onChange={(e) => setOpenaiApiKey(e.target.value)}
                        placeholder={hasOpenaiKey ? '********' : 'sk-...'}
                        className="form-input"
                      />
                    </div>
                    <div className="form-group">
                      <label htmlFor="openai-base">Base URL (Optional)</label>
                      <input
                        type="text"
                        id="openai-base"
                        value={openaiBaseUrl}
                        onChange={(e) => setOpenaiBaseUrl(e.target.value)}
                        placeholder="https://api.openai.com/v1"
                        className="form-input"
                      />
                    </div>
                  </div>
                )}

                {/* Anthropic settings */}
                {provider === 'anthropic' && (
                  <div className="settings-card">
                    <h3 className="card-title">Anthropic Configuration</h3>
                    <div className="form-group">
                      <label htmlFor="anthropic-key">
                        API Key {hasAnthropicKey && <span className="credential-set">(set)</span>}
                      </label>
                      <input
                        type="password"
                        id="anthropic-key"
                        value={anthropicApiKey}
                        onChange={(e) => setAnthropicApiKey(e.target.value)}
                        placeholder={hasAnthropicKey ? '********' : 'sk-ant-...'}
                        className="form-input"
                      />
                    </div>
                    <div className="form-group">
                      <label htmlFor="anthropic-base">Base URL (Optional)</label>
                      <input
                        type="text"
                        id="anthropic-base"
                        value={anthropicBaseUrl}
                        onChange={(e) => setAnthropicBaseUrl(e.target.value)}
                        placeholder="https://api.anthropic.com"
                        className="form-input"
                      />
                    </div>
                    <div className="alert alert-warning">
                      Anthropic does not provide embedding models. Use a different provider for
                      embeddings.
                    </div>
                  </div>
                )}

                {/* Bedrock settings */}
                {provider === 'bedrock' && (
                  <div className="settings-card">
                    <h3 className="card-title">AWS Bedrock Configuration</h3>
                    <div className="form-group">
                      <label htmlFor="bedrock-region">AWS Region</label>
                      <select
                        id="bedrock-region"
                        value={bedrockRegion}
                        onChange={(e) => setBedrockRegion(e.target.value)}
                        className="form-select"
                      >
                        {awsRegions.map((region) => (
                          <option key={region} value={region}>
                            {region}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="form-group">
                      <label className="checkbox-label">
                        <input
                          type="checkbox"
                          checked={bedrockUseIamRole}
                          onChange={(e) => setBedrockUseIamRole(e.target.checked)}
                        />
                        Use IAM Role (for EC2/ECS)
                      </label>
                    </div>
                    {!bedrockUseIamRole && (
                      <>
                        <div className="form-group">
                          <label htmlFor="bedrock-access">
                            Access Key{' '}
                            {hasBedrockCreds && <span className="credential-set">(set)</span>}
                          </label>
                          <input
                            type="text"
                            id="bedrock-access"
                            value={bedrockAccessKey}
                            onChange={(e) => setBedrockAccessKey(e.target.value)}
                            placeholder={hasBedrockCreds ? '********' : 'AKIA...'}
                            className="form-input"
                          />
                        </div>
                        <div className="form-group">
                          <label htmlFor="bedrock-secret">Secret Key</label>
                          <input
                            type="password"
                            id="bedrock-secret"
                            value={bedrockSecretKey}
                            onChange={(e) => setBedrockSecretKey(e.target.value)}
                            placeholder={hasBedrockCreds ? '********' : ''}
                            className="form-input"
                          />
                        </div>
                      </>
                    )}
                  </div>
                )}

                {/* Model Selection */}
                <div className="settings-card">
                  <h3 className="card-title">Default Models</h3>
                  <div className="form-group">
                    <label htmlFor="model-fast">Fast Model</label>
                    <select
                      id="model-fast"
                      value={modelFast}
                      onChange={(e) => setModelFast(e.target.value)}
                      className="form-select"
                    >
                      <option value="">Select a model</option>
                      {currentModels.fast.map((model) => (
                        <option key={model} value={model}>
                          {model}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div className="form-group">
                    <label htmlFor="model-reasoning">Reasoning Model</label>
                    <select
                      id="model-reasoning"
                      value={modelReasoning}
                      onChange={(e) => setModelReasoning(e.target.value)}
                      className="form-select"
                    >
                      <option value="">Select a model</option>
                      {currentModels.reasoning.map((model) => (
                        <option key={model} value={model}>
                          {model}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div className="form-group">
                    <label htmlFor="model-embedding">Embedding Model</label>
                    {currentModels.embedding.length > 0 ? (
                      <select
                        id="model-embedding"
                        value={modelEmbedding}
                        onChange={(e) => setModelEmbedding(e.target.value)}
                        className="form-select"
                      >
                        <option value="">Select a model</option>
                        {currentModels.embedding.map((model) => (
                          <option key={model} value={model}>
                            {model}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <input
                        type="text"
                        id="model-embedding"
                        value={modelEmbedding}
                        onChange={(e) => setModelEmbedding(e.target.value)}
                        placeholder="text-embedding-3-small (from another provider)"
                        className="form-input"
                      />
                    )}
                  </div>
                  <div className="form-group">
                    <label htmlFor="model-image">Image Model</label>
                    <select
                      id="model-image"
                      value={modelImage}
                      onChange={(e) => setModelImage(e.target.value)}
                      className="form-select"
                    >
                      <option value="">Use server default</option>
                      {Array.from(
                        new Set([...IMAGE_MODEL_OPTIONS, modelImage].filter(Boolean))
                      ).map((model) => (
                        <option key={model} value={model}>
                          {model}
                        </option>
                      ))}
                    </select>
                    <p className="form-hint">
                      ComfyUI checkpoint used when a message asks for an image. Leave empty to use
                      COMFYUI_CHECKPOINT.
                    </p>
                  </div>
                </div>
              </section>

              {/* Actions */}
              <div className="settings-actions">
                <Button type="button" onClick={handleReset} disabled={saving} variant="secondary">
                  Reset to Defaults
                </Button>
                <Button type="submit" loading={saving} variant="primary">
                  {saving ? 'Saving...' : 'Save Changes'}
                </Button>
              </div>
            </form>
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
