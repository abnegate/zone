import { Button, TabsContent, TabsList, TabsTrigger } from '@zone/ui';
import {
  type CSSProperties,
  type FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { client } from '../../../../api/client';
import { useTheme, workspaceThemeProperties } from '../../../../shared/context/ThemeContext';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useAuth } from '../../../auth';
import { useModels } from '../../../models';
import { mergeStageOptions } from '../../../models/utils/stageOptions';
import {
  AiModelFields,
  AiProviderFields,
  buildAiSettingsRequest,
  configuredFromSettings,
  credentialsFromSettings,
  emptyCredentials,
  emptyModels,
  hasOverrides,
  type ModelSelection,
  modelOptions,
  modelsFromSettings,
  nothingConfigured,
  type ProviderConfigured,
  type ProviderCredentials,
  providerOptions,
} from '../../ai';
import { SettingsPage } from '../../components';
import { WorkspaceMembersSection } from '../components';
import { UpdateWorkspaceThemeRequestSchema } from '../schemas';
import type {
  AiProvider,
  AiSettings,
  BorderRadius,
  FontFamily,
  UpdateWorkspaceThemeRequest,
  WorkspaceTheme,
} from '../types';
import './WorkspaceSettingsPage.css';

type Tab = 'theme' | 'ai' | 'members';

const TITLE = 'Workspace Settings';

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

const DEFAULT_PRIMARY = '#3b82f6';
const DEFAULT_SECONDARY = '#6366f1';
const HEX_PATTERN = '^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$';

function ColorField({
  id,
  label,
  value,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="form-group">
      <label htmlFor={id}>{label}</label>
      <div className="color-input-wrapper">
        <input type="color" id={id} value={value} onChange={(e) => onChange(e.target.value)} />
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          pattern={HEX_PATTERN}
          className="color-text-input"
          aria-label={`${label} hex`}
        />
      </div>
    </div>
  );
}

export default function WorkspaceSettingsPage() {
  const { isAuthenticated } = useAuth();
  const { models: installedModels } = useModels();
  const {
    theme,
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
  const touched = useRef(new Set<keyof UpdateWorkspaceThemeRequest>());
  const scope = `${orgId}/${workspaceId}`;
  const currentScope = useRef<string | null>(scope);
  currentScope.current = scope;
  const loading = activeTab === 'theme' ? workspaceThemeLoading : aiLoading;
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const [primaryColorLight, setPrimaryColorLight] = useState(DEFAULT_PRIMARY);
  const [secondaryColorLight, setSecondaryColorLight] = useState(DEFAULT_SECONDARY);
  const [primaryColorDark, setPrimaryColorDark] = useState(DEFAULT_PRIMARY);
  const [secondaryColorDark, setSecondaryColorDark] = useState(DEFAULT_SECONDARY);
  const [fontFamily, setFontFamily] = useState<FontFamily | null>(null);
  const [fontSize, setFontSize] = useState('16');
  const [borderRadius, setBorderRadius] = useState<BorderRadius | null>(null);

  const [overrideAiSettings, setOverrideAiSettings] = useState(false);
  const [aiProvider, setAiProvider] = useState<AiProvider>('self_hosted');
  const [credentials, setCredentials] = useState<ProviderCredentials>(emptyCredentials);
  const [configured, setConfigured] = useState<ProviderConfigured>(nothingConfigured);
  const [models, setModels] = useState<ModelSelection>(emptyModels);
  const [effectiveSettings, setEffectiveSettings] = useState<AiSettings | null>(null);

  const applyAiSettingsToForm = useCallback((settings: AiSettings): void => {
    setOverrideAiSettings(hasOverrides(settings));
    setAiProvider(settings.provider);
    setCredentials(credentialsFromSettings(settings));
    setConfigured(configuredFromSettings(settings));
    setModels(modelsFromSettings(settings));
  }, []);

  const applyThemeToForm = useCallback((theme: WorkspaceTheme | null): void => {
    setPrimaryColorLight(theme?.primary_color_light ?? DEFAULT_PRIMARY);
    setSecondaryColorLight(theme?.secondary_color_light ?? DEFAULT_SECONDARY);
    setPrimaryColorDark(theme?.primary_color_dark ?? DEFAULT_PRIMARY);
    setSecondaryColorDark(theme?.secondary_color_dark ?? DEFAULT_SECONDARY);
    setFontFamily(theme?.font_family ?? null);
    setFontSize((theme?.font_size_base ?? '16px').replace('px', ''));
    setBorderRadius(theme?.border_radius ?? null);
  }, []);

  useEffect(() => {
    applyThemeToForm(workspaceTheme?.workspace_id === workspaceId ? workspaceTheme : null);
    edited.current = false;
    touched.current.clear();
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
      field: keyof UpdateWorkspaceThemeRequest,
      value: T | null,
      previous: T | null | undefined
    ): T | null => (touched.current.has(field) ? value : (previous ?? null));
    return {
      primary_color_light: preserve(
        'primary_color_light',
        primaryColorLight,
        saved?.primary_color_light
      ),
      secondary_color_light: preserve(
        'secondary_color_light',
        secondaryColorLight,
        saved?.secondary_color_light
      ),
      primary_color_dark: preserve(
        'primary_color_dark',
        primaryColorDark,
        saved?.primary_color_dark
      ),
      secondary_color_dark: preserve(
        'secondary_color_dark',
        secondaryColorDark,
        saved?.secondary_color_dark
      ),
      font_family: preserve('font_family', fontFamily, saved?.font_family),
      font_size_base: preserve('font_size_base', `${fontSize}px`, saved?.font_size_base),
      border_radius: preserve('border_radius', borderRadius, saved?.border_radius),
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

  const previewStyle = useMemo(() => {
    const style: Record<string, string> = {};
    const draft: WorkspaceTheme = {
      workspace_id: workspaceId ?? '',
      primary_color_light: primaryColorLight,
      secondary_color_light: secondaryColorLight,
      primary_color_dark: primaryColorDark,
      secondary_color_dark: secondaryColorDark,
      font_family: fontFamily,
      font_size_base: `${fontSize}px`,
      border_radius: borderRadius,
      created_at: '',
      updated_at: '',
    };
    for (const [name, value] of workspaceThemeProperties(draft, theme)) {
      style[name === 'font-size' ? 'fontSize' : name] = value;
    }
    return style as CSSProperties;
  }, [
    workspaceId,
    primaryColorLight,
    secondaryColorLight,
    primaryColorDark,
    secondaryColorDark,
    fontFamily,
    fontSize,
    borderRadius,
    theme,
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

  const flash = (message: string) => {
    setSuccess(message);
    setTimeout(() => {
      if (currentScope.current === scope) setSuccess(null);
    }, 3000);
  };

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
        touched.current.clear();
        setDirty(false);
        previewWorkspaceTheme(null);
      }

      if (activeTab === 'ai' && overrideAiSettings) {
        const aiSettings = await client.updateWorkspaceAiSettings(
          orgId,
          workspaceId,
          buildAiSettingsRequest(aiProvider, credentials, models)
        );
        if (currentScope.current !== scope) return;
        applyAiSettingsToForm(aiSettings);
        const effective = await client.getEffectiveAiSettings(orgId, workspaceId);
        if (currentScope.current !== scope) return;
        setEffectiveSettings(effective);
      }

      flash('Settings saved successfully');
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
        touched.current.clear();
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
      flash('Settings reset to defaults');
    } catch (err) {
      if (currentScope.current === scope)
        setError(err instanceof Error ? err.message : 'Failed to reset settings');
    } finally {
      if (currentScope.current === scope) setSaving(false);
    }
  };

  const stage = modelOptions[aiProvider];
  const fastOptions = mergeStageOptions(stage.fast, installedModels, models.fast, 'chat');
  const reasoningOptions = mergeStageOptions(
    stage.reasoning,
    installedModels,
    models.reasoning,
    'chat'
  );
  const embeddingOptions = mergeStageOptions(
    stage.embedding,
    installedModels,
    models.embedding,
    'embedding'
  );

  const colorField = (
    field: keyof UpdateWorkspaceThemeRequest,
    set: (value: string) => void
  ): ((value: string) => void) => {
    return (value) => {
      touched.current.add(field);
      set(value);
    };
  };

  const tabs = (
    <TabsList aria-label="Workspace settings">
      <TabsTrigger value="theme">Theme</TabsTrigger>
      <TabsTrigger value="ai">AI Settings</TabsTrigger>
      <TabsTrigger value="members">Members</TabsTrigger>
    </TabsList>
  );
  const selectTab = (value: string) => setActiveTab(value as Tab);

  if (loading) {
    return (
      <SettingsPage title={TITLE} tabs={tabs} value={activeTab} onValueChange={selectTab}>
        <div className="loading-state">
          {activeTab === 'theme' ? 'Loading theme settings...' : 'Loading AI settings...'}
        </div>
      </SettingsPage>
    );
  }

  if (!orgId || !workspaceId) {
    return (
      <SettingsPage title={TITLE}>
        <div className="alert alert-error">
          No workspace selected. Please select or create a workspace first.
        </div>
      </SettingsPage>
    );
  }

  const actions = (
    <div className="settings-actions">
      <Button type="button" variant="ghost" size="sm" onClick={handleReset} disabled={saving}>
        Reset to Defaults
      </Button>
      <Button type="submit" loading={saving}>
        {saving ? 'Saving...' : 'Save Changes'}
      </Button>
    </div>
  );

  const effectiveRows: [string, string][] = effectiveSettings
    ? [
        [
          'Provider',
          providerOptions.find((option) => option.value === effectiveSettings.provider)?.label ||
            effectiveSettings.provider,
        ],
        ['Fast Model', effectiveSettings.model_fast || 'Not configured'],
        ['Reasoning Model', effectiveSettings.model_reasoning || 'Not configured'],
        ['Embedding Model', effectiveSettings.model_embedding || 'Not configured'],
        ['Image Model', effectiveSettings.model_image || 'Server default'],
        ['Video Model', effectiveSettings.model_video || 'Server default'],
        ['Audio Model', effectiveSettings.model_audio || 'Server default'],
      ]
    : [];

  return (
    <SettingsPage title={TITLE} tabs={tabs} value={activeTab} onValueChange={selectTab}>
      {(error || (activeTab === 'theme' && workspaceThemeError)) && (
        <div className="alert alert-error">{error || workspaceThemeError}</div>
      )}
      {success && <div className="alert alert-success">{success}</div>}

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
          <div className="section-row">
            <div className="section-row-copy">
              <h2 className="section-title">Theme Configuration</h2>
              <p className="section-description">
                Changes preview live and apply to everyone in this workspace once saved.
              </p>
            </div>
          </div>

          <div className="settings-card">
            <h3 className="card-title">Colors</h3>
            <div className="theme-groups">
              <div className="theme-group">
                <h4 className="settings-eyebrow">Light Mode Colors</h4>
                <div className="form-grid">
                  <ColorField
                    id="primary-light"
                    label="Primary Color"
                    value={primaryColorLight}
                    onChange={colorField('primary_color_light', setPrimaryColorLight)}
                  />
                  <ColorField
                    id="secondary-light"
                    label="Secondary Color"
                    value={secondaryColorLight}
                    onChange={colorField('secondary_color_light', setSecondaryColorLight)}
                  />
                </div>
              </div>
              <div className="theme-group">
                <h4 className="settings-eyebrow">Dark Mode Colors</h4>
                <div className="form-grid">
                  <ColorField
                    id="primary-dark"
                    label="Primary Color"
                    value={primaryColorDark}
                    onChange={colorField('primary_color_dark', setPrimaryColorDark)}
                  />
                  <ColorField
                    id="secondary-dark"
                    label="Secondary Color"
                    value={secondaryColorDark}
                    onChange={colorField('secondary_color_dark', setSecondaryColorDark)}
                  />
                </div>
              </div>
            </div>
          </div>

          <div className="settings-card">
            <h3 className="card-title">Typography &amp; shape</h3>
            <div className="form-grid">
              <div className="form-group">
                <label htmlFor="font-family">Font Family</label>
                <select
                  id="font-family"
                  value={fontFamily ?? ''}
                  onChange={(e) => {
                    touched.current.add('font_family');
                    setFontFamily(e.target.value as FontFamily);
                  }}
                  className="form-select"
                >
                  <option value="" disabled>
                    App Default
                  </option>
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
                    onChange={(e) => {
                      touched.current.add('font_size_base');
                      setFontSize(e.target.value);
                    }}
                    className="form-slider"
                  />
                  <span className="slider-value">{fontSize}px</span>
                </div>
              </div>
              <div className="form-group form-group--full">
                <span className="form-label">Corner Radius</span>
                <div className="radio-group">
                  {borderRadius === null && (
                    <label className="radio-option">
                      <input type="radio" name="border-radius" value="" checked disabled />
                      <span className="radio-label">App Default</span>
                    </label>
                  )}
                  {radiusOptions.map((opt) => (
                    <label key={opt.value} className="radio-option">
                      <input
                        type="radio"
                        name="border-radius"
                        value={opt.value}
                        checked={borderRadius === opt.value}
                        onChange={() => {
                          touched.current.add('border_radius');
                          setBorderRadius(opt.value);
                        }}
                      />
                      <span className="radio-label">{opt.label}</span>
                    </label>
                  ))}
                </div>
              </div>
            </div>
          </div>

          <div className="preview-box" style={previewStyle}>
            <h3 className="settings-eyebrow">Preview</h3>
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

          {actions}
        </form>
      </TabsContent>

      <TabsContent value="ai">
        <form onSubmit={handleSave} className="settings-form">
          <div className="section-row">
            <div className="section-row-copy">
              <h2 className="section-title">AI Provider Settings</h2>
              <p className="section-description">
                Inherits the organization's provider and models unless overridden here.
              </p>
            </div>
          </div>

          <div className="settings-card">
            <div className="toggle-row">
              <input
                type="checkbox"
                id="override-ai-settings"
                aria-describedby="override-ai-settings-hint"
                checked={overrideAiSettings}
                onChange={(e) => setOverrideAiSettings(e.target.checked)}
              />
              <label htmlFor="override-ai-settings" className="toggle-row-label">
                Override organization AI settings
              </label>
              <p id="override-ai-settings-hint" className="toggle-row-description">
                When disabled, this workspace uses the organization's AI provider settings.
              </p>
            </div>

            {overrideAiSettings ? (
              <AiProviderFields
                provider={aiProvider}
                onProviderChange={(provider) => {
                  setAiProvider(provider);
                  setModels((prev) => ({ ...prev, fast: '', reasoning: '', embedding: '' }));
                }}
                credentials={credentials}
                configured={configured}
                onChange={(key, value) => setCredentials((prev) => ({ ...prev, [key]: value }))}
              />
            ) : (
              <div className="effective-block">
                <h3 className="settings-eyebrow">Effective Settings (from Organization)</h3>
                {effectiveSettings ? (
                  <div className="effective-settings">
                    {effectiveRows.map(([label, value]) => (
                      <div key={label} className="effective-row">
                        <span className="effective-label">{label}</span>
                        <span className="effective-value">{value}</span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="form-hint">No organization settings configured.</p>
                )}
              </div>
            )}
          </div>

          {overrideAiSettings && (
            <div className="settings-card">
              <h3 className="card-title">Default Models</h3>
              <AiModelFields
                provider={aiProvider}
                models={models}
                onChange={(key, value) => setModels((prev) => ({ ...prev, [key]: value }))}
                fastOptions={fastOptions}
                reasoningOptions={reasoningOptions}
                embeddingOptions={embeddingOptions}
                installedModels={installedModels}
                inheritedLabel="Use organization / server default"
              />
            </div>
          )}

          {actions}
        </form>
      </TabsContent>
    </SettingsPage>
  );
}
