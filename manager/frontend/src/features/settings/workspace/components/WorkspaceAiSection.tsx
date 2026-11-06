import { Button, Select } from '@zone/ui';
import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { client } from '../../../../api/client';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useModels } from '../../../models';
import {
  AgentSignIn,
  AiProviderFields,
  AUDIO_MODEL_OPTIONS,
  agentAccess,
  agentOf,
  configuredFromSettings,
  credentialsFromSettings,
  emptyCredentials,
  imageModelOptions,
  type ModelOption,
  modelChoices,
  modelsFromSettings,
  type ProviderCredentials,
  providerOptions,
  useAgentStatuses,
  VIDEO_MODEL_OPTIONS,
  withCurrent,
} from '../../ai';
import {
  InheritableField,
  inheritsEverything,
  MODEL_FIELDS,
  type ModelField,
  modelOverrideRequest,
  modelResetRequest,
  providerOverrideRequest,
  providerResetRequest,
  sourceOf,
  workspaceProvider,
} from '../inheritance';
import type {
  AiProvider,
  AiSettings,
  UpdateAiSettingsRequest,
  WorkspaceAiSettings,
} from '../types';
import { InheritedField } from './InheritedField';

interface WorkspaceAiSectionProps {
  editable: boolean;
  organizationDefaultsPath?: string;
}

type Mutation = (organizationId: string, workspaceId: string) => Promise<WorkspaceAiSettings>;

const FIELD_LABELS: Record<InheritableField, string> = {
  [InheritableField.Provider]: 'Provider',
  [InheritableField.Fast]: 'Fast Model',
  [InheritableField.Reasoning]: 'Reasoning Model',
  [InheritableField.Embedding]: 'Embedding Model',
  [InheritableField.Image]: 'Image Model',
  [InheritableField.Video]: 'Video Model',
  [InheritableField.Audio]: 'Audio Model',
};

type ChatField = Extract<ModelField, 'fast' | 'reasoning' | 'embedding'>;

const CHAT_FIELDS: readonly ModelField[] = [
  InheritableField.Fast,
  InheritableField.Reasoning,
  InheritableField.Embedding,
];

const AUTOMATIC = 'Automatic';
const SERVER_DEFAULT = 'Server default';
const FLASH_DURATION = 3000;

function isChatField(field: ModelField): field is ChatField {
  return CHAT_FIELDS.includes(field);
}

function providerLabel(provider: AiProvider): string {
  return providerOptions.find((option) => option.value === provider)?.label ?? provider;
}

function modelOf(settings: AiSettings, field: ModelField): string | null {
  return settings[`model_${field}`];
}

function plain(options: string[]): ModelOption[] {
  return options.map((value) => ({ value, label: value }));
}

function reasonOf(failure: unknown, fallback: string): string {
  return failure instanceof Error ? failure.message : fallback;
}

export function WorkspaceAiSection({
  editable,
  organizationDefaultsPath,
}: WorkspaceAiSectionProps) {
  const { currentOrganization, currentWorkspace, resolvingRole } = useWorkspace();
  const { models: installedModels } = useModels();
  const organizationId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;
  const scope = `${organizationId}/${workspaceId}`;
  const currentScope = useRef(scope);
  currentScope.current = scope;
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [saved, setSaved] = useState<WorkspaceAiSettings | null>(null);
  const [effective, setEffective] = useState<AiSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [editing, setEditing] = useState<InheritableField | null>(null);
  const [draftModel, setDraftModel] = useState('');
  const [draftProvider, setDraftProvider] = useState<AiProvider>('self_hosted');
  const [credentials, setCredentials] = useState<ProviderCredentials>(emptyCredentials);

  const editingProvider = editing === InheritableField.Provider;
  const agent = editingProvider
    ? agentOf(draftProvider)
    : editing !== null && isChatField(editing) && effective
      ? agentOf(effective.provider)
      : null;
  const agents = useAgentStatuses(organizationId, agent !== null);
  const agentModels = agent ? (agents.statuses[agent]?.models ?? []) : [];

  useEffect(
    () => () => {
      if (flashTimer.current) clearTimeout(flashTimer.current);
    },
    []
  );

  useEffect(() => {
    setSaved(null);
    setEffective(null);
    setEditing(null);
    setBusy(false);
    setError(null);
    setSuccess(null);
    if (!organizationId || !workspaceId) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    Promise.all([
      client.getWorkspaceAiSettings(organizationId, workspaceId),
      client.getEffectiveAiSettings(organizationId, workspaceId),
    ])
      .then(([settings, merged]) => {
        if (cancelled) return;
        setSaved(settings);
        setEffective(merged);
      })
      .catch((failure: unknown) => {
        if (!cancelled) setError(reasonOf(failure, 'Failed to load AI settings'));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [organizationId, workspaceId]);

  const flash = useCallback((message: string) => {
    setSuccess(message);
    if (flashTimer.current) clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setSuccess(null), FLASH_DURATION);
  }, []);

  const mutate = async (mutation: Mutation, message: string, fallback: string): Promise<void> => {
    if (!organizationId || !workspaceId) return;
    const started = currentScope.current;
    const stale = () => currentScope.current !== started;
    setBusy(true);
    setError(null);
    setSuccess(null);
    try {
      let settings = await mutation(organizationId, workspaceId);
      if (stale()) return;
      if (settings.overrides && inheritsEverything(settings)) {
        settings = await client.resetWorkspaceAiSettings(organizationId, workspaceId);
        if (stale()) return;
      }
      setSaved(settings);
      setEditing(null);
      const merged = await client.getEffectiveAiSettings(organizationId, workspaceId);
      if (stale()) return;
      setEffective(merged);
      flash(message);
    } catch (failure) {
      if (!stale()) setError(reasonOf(failure, fallback));
    } finally {
      if (!stale()) setBusy(false);
    }
  };

  const save = (request: UpdateAiSettingsRequest, message: string) =>
    mutate(
      (organization, workspace) =>
        client.updateWorkspaceAiSettings(organization, workspace, request),
      message,
      'Failed to save AI settings'
    );

  const resetAll = () =>
    mutate(
      (organization, workspace) => client.resetWorkspaceAiSettings(organization, workspace),
      'AI settings follow the organization',
      'Failed to reset AI settings'
    );

  if (loading) {
    return <div className="loading-state">Loading AI settings...</div>;
  }

  if (!organizationId || !workspaceId) {
    return (
      <div className="alert alert-error">
        No workspace selected. Please select or create a workspace first.
      </div>
    );
  }

  const startEditing = (field: InheritableField) => {
    if (!saved || !effective) return;
    setError(null);
    if (field === InheritableField.Provider) {
      setDraftProvider(workspaceProvider(saved) ?? effective.provider);
      setCredentials(credentialsFromSettings(saved));
    } else {
      setDraftModel(modelOf(saved, field) ?? modelOf(effective, field) ?? '');
    }
    setEditing(field);
  };

  const modelOptions = (field: ModelField, merged: AiSettings): ModelOption[] => {
    if (isChatField(field)) {
      const selection = { ...modelsFromSettings(merged), [field]: draftModel };
      return plain(modelChoices(merged.provider, installedModels, selection, agentModels)[field]);
    }
    if (field === InheritableField.Image) return imageModelOptions(installedModels, draftModel);
    if (field === InheritableField.Video)
      return plain(withCurrent(VIDEO_MODEL_OPTIONS, draftModel));
    return plain(withCurrent(AUDIO_MODEL_OPTIONS, draftModel));
  };

  const editorActions = (label: string, onSave: () => void, disabled: boolean) => (
    <div className="inherited-field-editor-actions">
      <Button variant="ghost" size="sm" disabled={busy} onClick={() => setEditing(null)}>
        Cancel
      </Button>
      <Button
        size="sm"
        aria-label={`Save ${label}`}
        loading={busy}
        disabled={disabled}
        onClick={onSave}
      >
        Save
      </Button>
    </div>
  );

  const modelEditor = (field: ModelField, settings: WorkspaceAiSettings, merged: AiSettings) => {
    const label = FIELD_LABELS[field];
    const id = `override-${field}`;
    const options = modelOptions(field, merged);
    const value = draftModel.trim();
    return (
      <>
        {options.length > 0 ? (
          <Select
            id={id}
            label={label}
            value={draftModel}
            onValueChange={setDraftModel}
            options={options}
            wrapperClassName="form-group"
          />
        ) : (
          <div className="form-group">
            <label htmlFor={id}>{label}</label>
            <input
              type="text"
              id={id}
              value={draftModel}
              onChange={(event) => setDraftModel(event.target.value)}
              className="form-input"
            />
          </div>
        )}
        {editorActions(
          label,
          () => save(modelOverrideRequest(field, value, settings), `${label} saved`),
          busy || !value
        )}
      </>
    );
  };

  const providerEditor = (settings: WorkspaceAiSettings, merged: AiSettings) => {
    const label = FIELD_LABELS[InheritableField.Provider];
    const providerAgent = agentOf(draftProvider);
    return (
      <>
        <AiProviderFields
          level="workspace"
          provider={draftProvider}
          onProviderChange={setDraftProvider}
          credentials={credentials}
          configured={configuredFromSettings(settings)}
          onChange={(key, value) => setCredentials((previous) => ({ ...previous, [key]: value }))}
          saved={settings}
          organizationKeys={settings.organization_keys}
        />
        {providerAgent && (
          <AgentSignIn
            key={`${organizationId}:${providerAgent}`}
            organizationId={organizationId}
            agent={providerAgent}
            access={agentAccess(currentOrganization?.role, resolvingRole)}
            heading="h3"
            unsaved={draftProvider !== workspaceProvider(settings)}
            status={agents.statuses[providerAgent]}
            attempt={agents.attempts[providerAgent]}
            loadError={agents.error}
            onStatusChange={agents.update}
            onAttemptChange={agents.setAttempt}
          />
        )}
        {editorActions(
          label,
          () =>
            save(
              providerOverrideRequest(draftProvider, credentials, settings, merged.provider),
              `${label} saved`
            ),
          busy
        )}
      </>
    );
  };

  const row = (
    field: InheritableField,
    value: string,
    settings: WorkspaceAiSettings,
    editor: () => ReactNode,
    resetRequest: () => UpdateAiSettingsRequest
  ) => {
    const label = FIELD_LABELS[field];
    const isEditing = editing === field;
    return (
      <InheritedField
        key={field}
        label={label}
        value={value}
        source={sourceOf(field, settings)}
        editable={editable}
        editing={isEditing}
        busy={busy}
        onOverride={() => startEditing(field)}
        onReset={() => save(resetRequest(), `${label} follows the organization`)}
      >
        {isEditing && editor()}
      </InheritedField>
    );
  };

  const fallbackFor = (field: ModelField) => (isChatField(field) ? AUTOMATIC : SERVER_DEFAULT);

  const headerActions =
    organizationDefaultsPath || (editable && saved?.overrides) ? (
      <div className="section-row-actions">
        {organizationDefaultsPath && (
          <Link to={organizationDefaultsPath} className="ui-btn ui-btn-ghost ui-btn-sm">
            Edit organization defaults
          </Link>
        )}
        {editable && saved?.overrides && (
          <Button
            variant="ghost"
            size="sm"
            aria-label="Reset all AI settings to the organization"
            disabled={busy}
            onClick={resetAll}
          >
            Reset all to organization
          </Button>
        )}
      </div>
    ) : null;

  return (
    <section className="settings-section" aria-labelledby="workspace-ai-title">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id="workspace-ai-title" className="section-title">
            AI Provider Settings
          </h2>
          <p className="section-description">
            Each field follows the organization unless this workspace overrides it.
          </p>
        </div>
        {headerActions}
      </div>

      {!editable && <p className="form-hint">Only workspace admins can change AI settings.</p>}
      {error && <div className="alert alert-error">{error}</div>}
      {success && <div className="alert alert-success">{success}</div>}

      {saved && effective && (
        <div className="settings-card">
          {row(
            InheritableField.Provider,
            providerLabel(effective.provider),
            saved,
            () => providerEditor(saved, effective),
            () => providerResetRequest(saved)
          )}
          {MODEL_FIELDS.map((field) =>
            row(
              field,
              modelOf(effective, field) || fallbackFor(field),
              saved,
              () => modelEditor(field, saved, effective),
              () => modelResetRequest(field, saved)
            )
          )}
        </div>
      )}
    </section>
  );
}

export default WorkspaceAiSection;
