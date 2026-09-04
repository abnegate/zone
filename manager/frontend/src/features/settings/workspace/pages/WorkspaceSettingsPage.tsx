import { Button, Tabs, TabsContent, TabsList, TabsTrigger } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { client } from '../../../../api/client';
import { useTheme } from '../../../../shared/context/ThemeContext';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useAuth } from '../../../auth';
import { WorkspaceMembersSection } from '../components';
import { UpdateWorkspaceThemeRequestSchema } from '../schemas';
import type {
  AiProvider,
  AiSettings,
  BorderRadius,
  FontFamily,
  UpdateAiSettingsRequest,
  UpdateWorkspaceThemeRequest,
  WorkspaceTheme,
} from '../types';
import './WorkspaceSettingsPage.css';

type Tab = 'theme' | 'ai' | 'members';

const fontOptions: { value: FontFamily; label: string }[] = [
  { value: 'system', label: 'System Default' },
  { value: 'inter', label: 'Inter' },
  { value: 'roboto', label: 'Roboto' },
  { value: 'open-sans', label: 'Open Sans' },
  { value: 'lato', label: 'Lato' },
  { value: 'nunito', label: 'Nunito' },
];

const radiusOptions: { value: BorderRadius; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'small', label: 'Small' },
  { value: 'medium', label: 'Medium' },
  { value: 'large', label: 'Large' },
];

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
    fast: ['anthropic.claude-3-haiku-20240307-v1:0', 'amazon.nova-lite-v1:0'],
    reasoning: ['anthropic.claude-3-5-sonnet-20241022-v2:0', 'amazon.nova-pro-v1:0'],
    embedding: ['amazon.titan-embed-text-v2:0', 'amazon.titan-embed-text-v1'],
  },
};

const IMAGE_MODEL_OPTIONS = ['flux1-schnell-fp8.safetensors'];

const awsRegions = ['us-east-1', 'us-west-2', 'eu-west-1', 'eu-central-1', 'ap-northeast-1'];

export default function WorkspaceSettingsPage() {
  const { isAuthenticated } = useAuth();
  const {
    workspaceTheme,
    workspaceThemeLoading,
    workspaceThemeError,
    setWorkspaceTheme,
    previewWorkspaceTheme,
  } = useTheme();
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const orgId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;

  const [activeTab, setActiveTab] = useState<Tab>('theme');
  const [aiLoading, setAiLoading] = useState(false);
  const [dirty, setDirty] = useState(false);
  const edited = useRef(false);
  const scope = `${orgId}/${workspaceId}`;
  const currentScope = useRef<string | null>(scope);
  currentScope.current = scope;
  const loading = activeTab === 'theme' ? workspaceThemeLoading : aiLoading;
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Form state - Theme
  const [primaryColorLight, setPrimaryColorLight] = useState('#3b82f6');
  const [secondaryColorLight, setSecondaryColorLight] = useState('#6366f1');
  const [primaryColorDark, setPrimaryColorDark] = useState('#3b82f6');
  const [secondaryColorDark, setSecondaryColorDark] = useState('#6366f1');
  const [fontFamily, setFontFamily] = useState<FontFamily>('system');
  const [fontSize, setFontSize] = useState('16');
  const [borderRadius, setBorderRadius] = useState<BorderRadius>('medium');

  // Form state - AI Settings
  const [overrideAiSettings, setOverrideAiSettings] = useState(false);
  const [aiProvider, setAiProvider] = useState<AiProvider>('self_hosted');
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
  const [hasLitellmKey, setHasLitellmKey] = useState(false);
  const [hasOpenaiKey, setHasOpenaiKey] = useState(false);
  const [hasAnthropicKey, setHasAnthropicKey] = useState(false);
  const [hasBedrockCreds, setHasBedrockCreds] = useState(false);
  const [effectiveSettings, setEffectiveSettings] = useState<AiSettings | null>(null);

  const applyAiSettingsToForm = useCallback((settings: AiSettings): void => {
    // Check if workspace has custom settings (provider != default means override)
    const hasCustomSettings = !!(
      settings.has_litellm_key ||
      settings.has_openai_api_key ||
      settings.has_anthropic_api_key ||
      settings.has_bedrock_credentials ||
      settings.model_fast ||
      settings.model_reasoning ||
      settings.model_embedding ||
      settings.model_image ||
      settings.litellm_host ||
      settings.openai_base_url ||
      settings.anthropic_base_url ||
      settings.bedrock_region
    );
    setOverrideAiSettings(hasCustomSettings);
    setAiProvider(settings.provider);
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
    // Clear password fields
    setLitellmKey('');
    setOpenaiApiKey('');
    setAnthropicApiKey('');
    setBedrockAccessKey('');
    setBedrockSecretKey('');
  }, []);

  const applyThemeToForm = useCallback((theme: WorkspaceTheme | null): void => {
    setPrimaryColorLight(theme?.primary_color_light ?? '#3b82f6');
    setSecondaryColorLight(theme?.secondary_color_light ?? '#6366f1');
    setPrimaryColorDark(theme?.primary_color_dark ?? '#3b82f6');
    setSecondaryColorDark(theme?.secondary_color_dark ?? '#6366f1');
    setFontFamily(theme?.font_family ?? 'system');
    setFontSize((theme?.font_size_base ?? '16px').replace('px', ''));
    setBorderRadius(theme?.border_radius ?? 'medium');
  }, []);

  useEffect(() => {
    applyThemeToForm(workspaceTheme?.workspace_id === workspaceId ? workspaceTheme : null);
    edited.current = false;
    setDirty(false);
    previewWorkspaceTheme(null);
  }, [workspaceTheme, workspaceId, previewWorkspaceTheme, applyThemeToForm]);

  useEffect(() => () => previewWorkspaceTheme(null), [previewWorkspaceTheme]);

  useEffect(() => {
    currentScope.current = scope;
    setSaving(false);
    setError(null);
    setSuccess(null);
    return () => {
      currentScope.current = null;
    };
  }, [scope]);

  useEffect(() => {
    if (activeTab !== 'ai' || !isAuthenticated || !orgId || !workspaceId) return;
    let cancelled = false;
    setAiLoading(true);
    setError(null);
    Promise.all([
      client.getWorkspaceAiSettings(orgId, workspaceId),
      client.getEffectiveAiSettings(orgId, workspaceId),
    ])
      .then(([settings, effective]) => {
        if (cancelled) return;
        applyAiSettingsToForm(settings);
        setEffectiveSettings(effective);
      })
      .catch((failure: unknown) => {
        if (!cancelled)
          setError(failure instanceof Error ? failure.message : 'Failed to load AI settings');
      })
      .finally(() => {
        if (!cancelled) setAiLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeTab, isAuthenticated, orgId, workspaceId, applyAiSettingsToForm]);

  const createThemeRequest = useCallback((): Required<UpdateWorkspaceThemeRequest> => {
    const saved = workspaceTheme?.workspace_id === workspaceId ? workspaceTheme : null;
    const preserve = <T extends string>(
      value: T,
      previous: T | null | undefined,
      fallback: T
    ): T | null => (value === (previous ?? fallback) ? (previous ?? null) : value);
    return {
      primary_color_light: preserve(primaryColorLight, saved?.primary_color_light, '#3b82f6'),
      secondary_color_light: preserve(secondaryColorLight, saved?.secondary_color_light, '#6366f1'),
      primary_color_dark: preserve(primaryColorDark, saved?.primary_color_dark, '#3b82f6'),
      secondary_color_dark: preserve(secondaryColorDark, saved?.secondary_color_dark, '#6366f1'),
      font_family: preserve(fontFamily, saved?.font_family, 'system'),
      font_size_base: preserve(`${fontSize}px`, saved?.font_size_base, '16px'),
      border_radius: preserve(borderRadius, saved?.border_radius, 'medium'),
    };
  }, [
    workspaceTheme,
    workspaceId,
    primaryColorLight,
    secondaryColorLight,
    primaryColorDark,
    secondaryColorDark,
    fontFamily,
    fontSize,
    borderRadius,
  ]);

  useEffect(() => {
    if (!dirty || !edited.current || workspaceThemeLoading || !workspaceId) return;
    const request = createThemeRequest();
    const result = UpdateWorkspaceThemeRequestSchema.safeParse(request);
    if (!result.success) return;
    previewWorkspaceTheme({
      workspace_id: workspaceId,
      ...request,
      created_at: '',
      updated_at: '',
    });
  }, [dirty, workspaceThemeLoading, workspaceId, createThemeRequest, previewWorkspaceTheme]);

  const handleSave = async (e: FormEvent): Promise<void> => {
    e.preventDefault();
    if (!isAuthenticated || !orgId || !workspaceId) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      if (activeTab === 'theme') {
        const theme = await client.updateWorkspaceTheme(
          orgId,
          workspaceId,
          UpdateWorkspaceThemeRequestSchema.parse(createThemeRequest())
        );
        if (currentScope.current !== scope) return;
        setWorkspaceTheme(theme);
        applyThemeToForm(theme);
        edited.current = false;
        setDirty(false);
        previewWorkspaceTheme(null);
      }

      // Save AI settings if overriding
      if (activeTab === 'ai' && overrideAiSettings) {
        const aiRequest: UpdateAiSettingsRequest = {
          provider: aiProvider,
          model_fast: modelFast || undefined,
          model_reasoning: modelReasoning || undefined,
          model_embedding: modelEmbedding || undefined,
          model_image: modelImage || undefined,
        };
        if (aiProvider === 'self_hosted') {
          aiRequest.litellm_host = litellmHost || undefined;
          if (litellmKey) aiRequest.litellm_key = litellmKey;
        } else if (aiProvider === 'openai') {
          aiRequest.openai_base_url = openaiBaseUrl || undefined;
          if (openaiApiKey) aiRequest.openai_api_key = openaiApiKey;
        } else if (aiProvider === 'anthropic') {
          aiRequest.anthropic_base_url = anthropicBaseUrl || undefined;
          if (anthropicApiKey) aiRequest.anthropic_api_key = anthropicApiKey;
        } else if (aiProvider === 'bedrock') {
          aiRequest.bedrock_region = bedrockRegion || undefined;
          aiRequest.bedrock_use_iam_role = bedrockUseIamRole;
          if (!bedrockUseIamRole) {
            if (bedrockAccessKey) aiRequest.bedrock_access_key = bedrockAccessKey;
            if (bedrockSecretKey) aiRequest.bedrock_secret_key = bedrockSecretKey;
          }
        }
        const aiSettings = await client.updateWorkspaceAiSettings(orgId, workspaceId, aiRequest);
        if (currentScope.current !== scope) return;
        applyAiSettingsToForm(aiSettings);
        const effective = await client.getEffectiveAiSettings(orgId, workspaceId);
        if (currentScope.current !== scope) return;
        setEffectiveSettings(effective);
      }

      setSuccess('Settings saved successfully');
      setTimeout(() => {
        if (currentScope.current === scope) setSuccess(null);
      }, 3000);
    } catch (err) {
      if (currentScope.current === scope)
        setError(err instanceof Error ? err.message : 'Failed to save settings');
    } finally {
      if (currentScope.current === scope) setSaving(false);
    }
  };

  const handleReset = async (): Promise<void> => {
    if (!isAuthenticated || !orgId || !workspaceId) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      if (activeTab === 'theme') {
        await client.resetWorkspaceTheme(orgId, workspaceId);
        if (currentScope.current !== scope) return;
        setWorkspaceTheme(null);
        applyThemeToForm(null);
        edited.current = false;
        setDirty(false);
        previewWorkspaceTheme(null);
      } else if (activeTab === 'ai') {
        const settings = await client.resetWorkspaceAiSettings(orgId, workspaceId);
        if (currentScope.current !== scope) return;
        applyAiSettingsToForm(settings);
        setOverrideAiSettings(false);
        const effective = await client.getEffectiveAiSettings(orgId, workspaceId);
        if (currentScope.current !== scope) return;
        setEffectiveSettings(effective);
      }
      setSuccess('Settings reset to defaults');
      setTimeout(() => {
        if (currentScope.current === scope) setSuccess(null);
      }, 3000);
    } catch (err) {
      if (currentScope.current === scope)
        setError(err instanceof Error ? err.message : 'Failed to reset settings');
    } finally {
      if (currentScope.current === scope) setSaving(false);
    }
  };

  const currentModels = modelOptions[aiProvider];

  if (loading) {
    return (
      <div className="page page--workspace settings-page">
        <header className="settings-page-header">
          <h1 className="page-title">Workspace Settings</h1>
        </header>
        <div className="settings-page-body">
          <div className="loading-state">Loading theme settings...</div>
        </div>
      </div>
    );
  }

  if (!orgId || !workspaceId) {
    return (
      <div className="page page--workspace settings-page">
        <header className="settings-page-header">
          <h1 className="page-title">Workspace Settings</h1>
        </header>
        <div className="settings-page-body">
          <div className="alert alert-error">
            No workspace selected. Please select or create a workspace first.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="page page--workspace settings-page">
      <header className="settings-page-header">
        <h1 className="page-title">Workspace Settings</h1>
      </header>
      <div className="settings-page-body">
        {(error || (activeTab === 'theme' && workspaceThemeError)) && (
          <div className="alert alert-error">{error || workspaceThemeError}</div>
        )}
        {success && <div className="alert alert-success">{success}</div>}

        <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as Tab)}>
          <TabsList>
            <TabsTrigger value="theme">Theme</TabsTrigger>
            <TabsTrigger value="ai">AI Settings</TabsTrigger>
            <TabsTrigger value="members">Members</TabsTrigger>
          </TabsList>

          <TabsContent value="members">
            <WorkspaceMembersSection workspaceId={workspaceId} orgId={orgId} />
          </TabsContent>

          <TabsContent value="theme">
            <form
              onSubmit={handleSave}
              onChange={() => {
                edited.current = true;
                setDirty(true);
              }}
              className="settings-form"
            >
              <section className="settings-section">
                <h2 className="section-title">Theme Configuration</h2>

                <div className="settings-grid">
                  {/* Light Mode Colors */}
                  <div className="settings-card">
                    <h3 className="card-title">Light Mode Colors</h3>
                    <div className="form-group">
                      <label htmlFor="primary-light">Primary Color</label>
                      <div className="color-input-wrapper">
                        <input
                          type="color"
                          id="primary-light"
                          value={primaryColorLight}
                          onChange={(e) => setPrimaryColorLight(e.target.value)}
                        />
                        <input
                          type="text"
                          value={primaryColorLight}
                          onChange={(e) => setPrimaryColorLight(e.target.value)}
                          pattern="^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$"
                          className="color-text-input"
                        />
                      </div>
                    </div>
                    <div className="form-group">
                      <label htmlFor="secondary-light">Secondary Color</label>
                      <div className="color-input-wrapper">
                        <input
                          type="color"
                          id="secondary-light"
                          value={secondaryColorLight}
                          onChange={(e) => setSecondaryColorLight(e.target.value)}
                        />
                        <input
                          type="text"
                          value={secondaryColorLight}
                          onChange={(e) => setSecondaryColorLight(e.target.value)}
                          pattern="^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$"
                          className="color-text-input"
                        />
                      </div>
                    </div>
                  </div>

                  {/* Dark Mode Colors */}
                  <div className="settings-card">
                    <h3 className="card-title">Dark Mode Colors</h3>
                    <div className="form-group">
                      <label htmlFor="primary-dark">Primary Color</label>
                      <div className="color-input-wrapper">
                        <input
                          type="color"
                          id="primary-dark"
                          value={primaryColorDark}
                          onChange={(e) => setPrimaryColorDark(e.target.value)}
                        />
                        <input
                          type="text"
                          value={primaryColorDark}
                          onChange={(e) => setPrimaryColorDark(e.target.value)}
                          pattern="^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$"
                          className="color-text-input"
                        />
                      </div>
                    </div>
                    <div className="form-group">
                      <label htmlFor="secondary-dark">Secondary Color</label>
                      <div className="color-input-wrapper">
                        <input
                          type="color"
                          id="secondary-dark"
                          value={secondaryColorDark}
                          onChange={(e) => setSecondaryColorDark(e.target.value)}
                        />
                        <input
                          type="text"
                          value={secondaryColorDark}
                          onChange={(e) => setSecondaryColorDark(e.target.value)}
                          pattern="^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$"
                          className="color-text-input"
                        />
                      </div>
                    </div>
                  </div>
                </div>

                {/* Typography */}
                <div className="settings-card">
                  <h3 className="card-title">Typography</h3>
                  <div className="settings-row">
                    <div className="form-group">
                      <label htmlFor="font-family">Font Family</label>
                      <select
                        id="font-family"
                        value={fontFamily}
                        onChange={(e) => setFontFamily(e.target.value as FontFamily)}
                        className="form-select"
                      >
                        {fontOptions.map((opt) => (
                          <option key={opt.value} value={opt.value}>
                            {opt.label}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="form-group">
                      <label htmlFor="font-size">Base Font Size</label>
                      <div className="slider-input-wrapper">
                        <input
                          type="range"
                          id="font-size"
                          min="12"
                          max="20"
                          value={fontSize}
                          onChange={(e) => setFontSize(e.target.value)}
                          className="form-slider"
                        />
                        <span className="slider-value">{fontSize}px</span>
                      </div>
                    </div>
                  </div>
                </div>

                {/* Appearance */}
                <div className="settings-card">
                  <h3 className="card-title">Appearance</h3>
                  <div className="form-group">
                    <span className="form-label">Corner Radius</span>
                    <div className="radio-group">
                      {radiusOptions.map((opt) => (
                        <label key={opt.value} className="radio-option">
                          <input
                            type="radio"
                            name="border-radius"
                            value={opt.value}
                            checked={borderRadius === opt.value}
                            onChange={() => setBorderRadius(opt.value)}
                          />
                          <span className="radio-label">{opt.label}</span>
                        </label>
                      ))}
                    </div>
                  </div>
                </div>

                {/* Preview */}
                <div className="settings-card">
                  <h3 className="card-title">Preview</h3>
                  <div className="preview-box">
                    <p className="preview-text">
                      This is a preview of your theme settings. Changes are applied live.
                    </p>
                    <div className="preview-buttons">
                      <Button type="button" variant="primary">
                        Primary Button
                      </Button>
                      <Button type="button" variant="secondary">
                        Secondary Button
                      </Button>
                    </div>
                    <div className="preview-card">
                      <strong>Sample Card</strong>
                      <p>This card demonstrates the corner radius and colors.</p>
                    </div>
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

          <TabsContent value="ai">
            <form onSubmit={handleSave} className="settings-form">
              <section className="settings-section">
                <h2 className="section-title">AI Provider Settings</h2>

                <div className="settings-card">
                  <div className="form-group">
                    <label className="checkbox-label">
                      <input
                        type="checkbox"
                        checked={overrideAiSettings}
                        onChange={(e) => setOverrideAiSettings(e.target.checked)}
                      />
                      <span>Override organization AI settings</span>
                    </label>
                    <p className="form-hint">
                      When disabled, this workspace will use the organization's AI provider
                      settings.
                    </p>
                  </div>
                </div>

                {overrideAiSettings ? (
                  <>
                    {/* Provider Selection */}
                    <div className="settings-card">
                      <h3 className="card-title">Provider</h3>
                      <div className="form-group">
                        <label htmlFor="ai-provider">AI Provider</label>
                        <select
                          id="ai-provider"
                          value={aiProvider}
                          onChange={(e) => {
                            setAiProvider(e.target.value as AiProvider);
                            setModelFast('');
                            setModelReasoning('');
                            setModelEmbedding('');
                          }}
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

                    {/* Provider-specific Credentials */}
                    <div className="settings-card">
                      <h3 className="card-title">Credentials</h3>

                      {aiProvider === 'self_hosted' && (
                        <>
                          <div className="form-group">
                            <label htmlFor="litellm-host">LiteLLM Host</label>
                            <input
                              type="text"
                              id="litellm-host"
                              value={litellmHost}
                              onChange={(e) => setLitellmHost(e.target.value)}
                              placeholder="http://localhost:4000"
                              className="form-input"
                            />
                          </div>
                          <div className="form-group">
                            <label htmlFor="litellm-key">
                              LiteLLM API Key
                              {hasLitellmKey && (
                                <span className="credential-set"> (configured)</span>
                              )}
                            </label>
                            <input
                              type="password"
                              id="litellm-key"
                              value={litellmKey}
                              onChange={(e) => setLitellmKey(e.target.value)}
                              placeholder={hasLitellmKey ? '••••••••' : 'Enter API key'}
                              className="form-input"
                            />
                          </div>
                        </>
                      )}

                      {aiProvider === 'openai' && (
                        <>
                          <div className="form-group">
                            <label htmlFor="openai-key">
                              OpenAI API Key
                              {hasOpenaiKey && (
                                <span className="credential-set"> (configured)</span>
                              )}
                            </label>
                            <input
                              type="password"
                              id="openai-key"
                              value={openaiApiKey}
                              onChange={(e) => setOpenaiApiKey(e.target.value)}
                              placeholder={hasOpenaiKey ? '••••••••' : 'sk-...'}
                              className="form-input"
                            />
                          </div>
                          <div className="form-group">
                            <label htmlFor="openai-base-url">Base URL (optional)</label>
                            <input
                              type="text"
                              id="openai-base-url"
                              value={openaiBaseUrl}
                              onChange={(e) => setOpenaiBaseUrl(e.target.value)}
                              placeholder="https://api.openai.com/v1"
                              className="form-input"
                            />
                          </div>
                        </>
                      )}

                      {aiProvider === 'anthropic' && (
                        <>
                          <div className="form-group">
                            <label htmlFor="anthropic-key">
                              Anthropic API Key
                              {hasAnthropicKey && (
                                <span className="credential-set"> (configured)</span>
                              )}
                            </label>
                            <input
                              type="password"
                              id="anthropic-key"
                              value={anthropicApiKey}
                              onChange={(e) => setAnthropicApiKey(e.target.value)}
                              placeholder={hasAnthropicKey ? '••••••••' : 'sk-ant-...'}
                              className="form-input"
                            />
                          </div>
                          <div className="form-group">
                            <label htmlFor="anthropic-base-url">Base URL (optional)</label>
                            <input
                              type="text"
                              id="anthropic-base-url"
                              value={anthropicBaseUrl}
                              onChange={(e) => setAnthropicBaseUrl(e.target.value)}
                              placeholder="https://api.anthropic.com"
                              className="form-input"
                            />
                          </div>
                        </>
                      )}

                      {aiProvider === 'bedrock' && (
                        <>
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
                              <span>Use IAM Role (EC2 instance profile / ECS task role)</span>
                            </label>
                          </div>
                          {!bedrockUseIamRole && (
                            <>
                              <div className="form-group">
                                <label htmlFor="bedrock-access-key">
                                  Access Key ID
                                  {hasBedrockCreds && (
                                    <span className="credential-set"> (configured)</span>
                                  )}
                                </label>
                                <input
                                  type="password"
                                  id="bedrock-access-key"
                                  value={bedrockAccessKey}
                                  onChange={(e) => setBedrockAccessKey(e.target.value)}
                                  placeholder={hasBedrockCreds ? '••••••••' : 'AKIA...'}
                                  className="form-input"
                                />
                              </div>
                              <div className="form-group">
                                <label htmlFor="bedrock-secret-key">Secret Access Key</label>
                                <input
                                  type="password"
                                  id="bedrock-secret-key"
                                  value={bedrockSecretKey}
                                  onChange={(e) => setBedrockSecretKey(e.target.value)}
                                  placeholder={hasBedrockCreds ? '••••••••' : 'Secret key'}
                                  className="form-input"
                                />
                              </div>
                            </>
                          )}
                        </>
                      )}
                    </div>

                    {/* Model Selection */}
                    <div className="settings-card">
                      <h3 className="card-title">Default Models</h3>
                      <div className="settings-row">
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
                          <>
                            <input
                              type="text"
                              id="model-embedding"
                              value={modelEmbedding}
                              onChange={(e) => setModelEmbedding(e.target.value)}
                              placeholder="Enter embedding model name"
                              className="form-input"
                            />
                            <p className="form-hint">
                              {aiProvider === 'anthropic'
                                ? 'Anthropic does not provide embedding models. Use a model from another provider (e.g., OpenAI text-embedding-3-small).'
                                : 'Enter a custom embedding model name.'}
                            </p>
                          </>
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
                          <option value="">Use organization / server default</option>
                          {Array.from(
                            new Set([...IMAGE_MODEL_OPTIONS, modelImage].filter(Boolean))
                          ).map((model) => (
                            <option key={model} value={model}>
                              {model}
                            </option>
                          ))}
                        </select>
                        <p className="form-hint">
                          ComfyUI checkpoint used when a message asks for an image.
                        </p>
                      </div>
                    </div>
                  </>
                ) : (
                  <div className="settings-card">
                    <h3 className="card-title">Effective Settings (from Organization)</h3>
                    {effectiveSettings ? (
                      <div className="effective-settings">
                        <div className="effective-row">
                          <span className="effective-label">Provider:</span>
                          <span className="effective-value">
                            {providerOptions.find((p) => p.value === effectiveSettings.provider)
                              ?.label || effectiveSettings.provider}
                          </span>
                        </div>
                        <div className="effective-row">
                          <span className="effective-label">Fast Model:</span>
                          <span className="effective-value">
                            {effectiveSettings.model_fast || 'Not configured'}
                          </span>
                        </div>
                        <div className="effective-row">
                          <span className="effective-label">Reasoning Model:</span>
                          <span className="effective-value">
                            {effectiveSettings.model_reasoning || 'Not configured'}
                          </span>
                        </div>
                        <div className="effective-row">
                          <span className="effective-label">Embedding Model:</span>
                          <span className="effective-value">
                            {effectiveSettings.model_embedding || 'Not configured'}
                          </span>
                        </div>
                        <div className="effective-row">
                          <span className="effective-label">Image Model:</span>
                          <span className="effective-value">
                            {effectiveSettings.model_image || 'Server default'}
                          </span>
                        </div>
                      </div>
                    ) : (
                      <p className="form-hint">No organization settings configured.</p>
                    )}
                  </div>
                )}
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
