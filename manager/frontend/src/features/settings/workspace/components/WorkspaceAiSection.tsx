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
  CHAT_MODEL_FIELDS,
  credentialRemovals,
  FIELD_LABELS,
  FieldSource,
  InheritableField,
  MODEL_FIELDS,
  type ModelField,
  modelOverrideRequest,
  modelResetRequest,
  providerOverrideRequest,
  providerResetRemovals,
  providerResetRequest,
  runpodOverrideRequest,
  runpodResetRequest,
  sourceOf,
  workspaceCredentials,
  workspaceProvider,
} from '../inheritance';
import { AiProviderSchema } from '../schemas';
import type {
  AiProvider,
  AiSettings,
  CommonAiSettings,
  UpdateAiSettingsRequest,
  WorkspaceAiSettings,
} from '../types';
import InheritedField from './InheritedField';
import ResetDialog from './ResetDialog';

interface WorkspaceAiSectionProps {
  editable: boolean;
  organizationDefaultsPath?: string;
}

type Mutation = (organizationId: string, workspaceId: string) => Promise<WorkspaceAiSettings>;

interface PendingReset {
  title: string;
  removals: string[];
  confirmLabel: string;
  run: () => void;
}

const ROW_ORDER: readonly InheritableField[] = [
  InheritableField.Provider,
  ...MODEL_FIELDS,
  InheritableField.Runpod,
];

type ChatField = Extract<ModelField, 'fast' | 'reasoning' | 'embedding'>;

const CHAT_FIELDS: readonly InheritableField[] = CHAT_MODEL_FIELDS;

const AUTOMATIC = 'Automatic';
const SERVER_DEFAULT = 'Server default';
const KEY_SET = 'Set';
const KEY_NOT_SET = 'Not set';
const SAVE_LABEL = 'Save';
const FLASH_DURATION = 3000;

function isChatField(field: InheritableField): field is ChatField {
  return CHAT_FIELDS.includes(field);
}

function providerLabel(provider: AiProvider): string {
  return providerOptions.find((option) => option.value === provider)?.label ?? provider;
}

function modelOf(settings: CommonAiSettings, field: ModelField): string | null {
  return settings[`model_${field}`];
}

function plain(options: string[]): ModelOption[] {
  return options.map((value) => ({ value, label: value }));
}

function reasonOf(failure: unknown, fallback: string): string {
  return failure instanceof Error ? failure.message : fallback;
}

function refreshFailed(message: string): string {
  return `${message}, but the refreshed values couldn't load. Reload the page to see them.`;
}

export default function WorkspaceAiSection({
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
  const [warning, setWarning] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [editing, setEditing] = useState<InheritableField | null>(null);
  const [pendingReset, setPendingReset] = useState<PendingReset | null>(null);
  const [draftModel, setDraftModel] = useState('');
  const [draftKey, setDraftKey] = useState('');
  const [draftProvider, setDraftProvider] = useState<AiProvider>(AiProviderSchema.enum.self_hosted);
  const [credentials, setCredentials] = useState<ProviderCredentials>(emptyCredentials);
  const [prefilled, setPrefilled] = useState<ProviderCredentials>(emptyCredentials);

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
    setPendingReset(null);
    setBusy(false);
    setError(null);
    setWarning(null);
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
    setWarning(null);
    setSuccess(null);
    try {
      const settings = await mutation(organizationId, workspaceId);
      if (stale()) return;
      setSaved(settings);
      setEditing(null);
    } catch (failure) {
      if (!stale()) {
        setError(reasonOf(failure, fallback));
        setBusy(false);
      }
      return;
    }
    try {
      const merged = await client.getEffectiveAiSettings(organizationId, workspaceId);
      if (stale()) return;
      setEffective(merged);
      flash(message);
    } catch {
      if (!stale()) setWarning(refreshFailed(message));
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

  const confirmReset = () => {
    const reset = pendingReset;
    setPendingReset(null);
    reset?.run();
  };

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
      const initial = workspaceCredentials(saved, effective);
      setDraftProvider(workspaceProvider(saved) ?? effective.provider);
      setCredentials(initial);
      setPrefilled(initial);
    } else if (field === InheritableField.Runpod) {
      setDraftKey('');
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
        aria-label={`${SAVE_LABEL} ${label}`}
        loading={busy}
        disabled={disabled}
        onClick={onSave}
      >
        {SAVE_LABEL}
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

  const runpodEditor = (settings: WorkspaceAiSettings) => {
    const label = FIELD_LABELS[InheritableField.Runpod];
    const id = 'override-runpod';
    const value = draftKey.trim();
    return (
      <>
        <div className="form-group">
          <label htmlFor={id}>{label}</label>
          <input
            type="password"
            id={id}
            value={draftKey}
            onChange={(event) => setDraftKey(event.target.value)}
            placeholder="Enter API key"
            autoComplete="off"
            className="form-input"
          />
        </div>
        {editorActions(
          label,
          () => save(runpodOverrideRequest(value, settings), `${label} saved`),
          busy || !value
        )}
      </>
    );
  };

  const providerEditor = (settings: WorkspaceAiSettings, merged: AiSettings) => {
    const label = FIELD_LABELS[InheritableField.Provider];
    const providerAgent = agentOf(draftProvider);
    const current = workspaceProvider(settings) ?? merged.provider;
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
            level="workspace"
            unsaved={draftProvider !== current}
            saveLabel={SAVE_LABEL}
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
              providerOverrideRequest(
                draftProvider,
                credentials,
                prefilled,
                settings,
                merged.provider
              ),
              `${label} saved`
            ),
          busy
        )}
      </>
    );
  };

  const resetField = (field: InheritableField, settings: WorkspaceAiSettings) => {
    const label = FIELD_LABELS[field];
    const message = `${label} follows the organization`;
    const confirm = (removals: string[], request: UpdateAiSettingsRequest) => {
      const run = () => save(request, message);
      if (removals.length === 0) {
        void run();
        return;
      }
      setPendingReset({
        title: `Reset ${label} to the organization?`,
        removals,
        confirmLabel: 'Reset',
        run,
      });
    };
    if (field === InheritableField.Provider) {
      confirm(providerResetRemovals(settings), providerResetRequest(settings));
    } else if (field === InheritableField.Runpod) {
      confirm([label], runpodResetRequest(settings));
    } else {
      void save(modelResetRequest(field, settings), message);
    }
  };

  const requestResetAll = (settings: WorkspaceAiSettings) => {
    const overridden = ROW_ORDER.filter(
      (field) => sourceOf(field, settings) === FieldSource.Workspace
    ).map((field) => FIELD_LABELS[field]);
    setPendingReset({
      title: 'Reset all AI settings to the organization?',
      removals: [...overridden, ...credentialRemovals(settings)],
      confirmLabel: 'Reset all',
      run: () => void resetAll(),
    });
  };

  const shownValue = (field: InheritableField, merged: AiSettings): string => {
    if (field === InheritableField.Provider) return providerLabel(merged.provider);
    if (field === InheritableField.Runpod) return merged.has_runpod_api_key ? KEY_SET : KEY_NOT_SET;
    return modelOf(merged, field) || (isChatField(field) ? AUTOMATIC : SERVER_DEFAULT);
  };

  const editorOf = (field: InheritableField, settings: WorkspaceAiSettings, merged: AiSettings) => {
    if (field === InheritableField.Provider) return providerEditor(settings, merged);
    if (field === InheritableField.Runpod) return runpodEditor(settings);
    return modelEditor(field, settings, merged);
  };

  const row = (field: InheritableField, settings: WorkspaceAiSettings, merged: AiSettings) => {
    const isEditing = editing === field;
    const elsewhere = editing !== null && !isEditing;
    const editor: ReactNode = isEditing ? editorOf(field, settings, merged) : null;
    return (
      <InheritedField
        key={field}
        label={FIELD_LABELS[field]}
        value={shownValue(field, merged)}
        source={sourceOf(field, settings)}
        editable={editable}
        editing={isEditing}
        busy={busy || elsewhere}
        onOverride={() => startEditing(field)}
        onReset={() => resetField(field, settings)}
      >
        {editor}
      </InheritedField>
    );
  };

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
            disabled={busy || editing !== null}
            onClick={() => requestResetAll(saved)}
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
      {warning && <div className="alert alert-warning">{warning}</div>}
      {success && <div className="alert alert-success">{success}</div>}

      {saved && effective && (
        <div className="settings-card">
          {ROW_ORDER.map((field) => row(field, saved, effective))}
        </div>
      )}

      <ResetDialog
        open={pendingReset !== null}
        title={pendingReset?.title ?? ''}
        removals={pendingReset?.removals ?? []}
        confirmLabel={pendingReset?.confirmLabel ?? ''}
        onConfirm={confirmReset}
        onClose={() => setPendingReset(null)}
      />
    </section>
  );
}
