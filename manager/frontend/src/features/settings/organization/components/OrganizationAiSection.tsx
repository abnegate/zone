import { Button } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useAuth } from '../../../auth';
import { useModels } from '../../../models';
import {
  AgentSignIn,
  AiModelFields,
  AiProviderFields,
  agentAccess,
  agentOf,
  buildAiSettingsRequest,
  configuredFromSettings,
  credentialsFromSettings,
  emptyCredentials,
  emptyModels,
  type ModelSelection,
  modelChoices,
  modelsFromSettings,
  nothingConfigured,
  type ProviderConfigured,
  type ProviderCredentials,
  useAgentStatuses,
} from '../../ai';
import type { AiProvider, AiSettings } from '../types';

const SUCCESS_DURATION_MS = 3000;
const SAVE_LABEL = 'Save Changes';

export function OrganizationAiSection() {
  const { isAuthenticated } = useAuth();
  const { currentOrganization, resolvingRole } = useWorkspace();
  const { models: installedModels } = useModels();

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [provider, setProvider] = useState<AiProvider>('self_hosted');
  const [savedProvider, setSavedProvider] = useState<AiProvider>('self_hosted');
  const [credentials, setCredentials] = useState<ProviderCredentials>(emptyCredentials);
  const [configured, setConfigured] = useState<ProviderConfigured>(nothingConfigured);
  const [models, setModels] = useState<ModelSelection>(emptyModels);
  const [saved, setSaved] = useState<AiSettings | null>(null);

  const agent = agentOf(provider);
  const agents = useAgentStatuses(currentOrganization?.id ?? null, agent !== null);

  const applySettingsToForm = useCallback((settings: AiSettings) => {
    setProvider(settings.provider);
    setSavedProvider(settings.provider);
    setCredentials(credentialsFromSettings(settings));
    setConfigured(configuredFromSettings(settings));
    setModels(modelsFromSettings(settings));
    setSaved(settings);
  }, []);

  const loadSettings = useCallback(async () => {
    if (!isAuthenticated || !currentOrganization) return;
    setLoading(true);
    setError(null);
    try {
      applySettingsToForm(await client.getOrgAiSettings(currentOrganization.id));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to load settings');
    } finally {
      setLoading(false);
    }
  }, [isAuthenticated, currentOrganization, applySettingsToForm]);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  const flash = (message: string) => {
    setSuccess(message);
    setTimeout(() => setSuccess(null), SUCCESS_DURATION_MS);
  };

  const handleSave = async (event: FormEvent) => {
    event.preventDefault();
    if (!isAuthenticated || !currentOrganization) return;

    setSaving(true);
    setError(null);
    setSuccess(null);
    setNotice(null);

    try {
      const settings = await client.updateOrgAiSettings(
        currentOrganization.id,
        buildAiSettingsRequest(provider, credentials, models, saved)
      );
      applySettingsToForm(settings);
      setNotice(settings.notice ?? null);
      flash('Settings saved successfully');
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to save settings');
    } finally {
      setSaving(false);
    }
  };

  const handleReset = async () => {
    if (!isAuthenticated || !currentOrganization) return;

    setSaving(true);
    setError(null);
    setSuccess(null);
    setNotice(null);

    try {
      const settings = await client.resetOrgAiSettings(currentOrganization.id);
      applySettingsToForm(settings);
      setNotice(settings.notice ?? null);
      flash('Settings reset to defaults');
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to reset settings');
    } finally {
      setSaving(false);
    }
  };

  if (!currentOrganization) {
    return <div className="loading-state">Please select an organization</div>;
  }

  if (loading) {
    return <div className="loading-state">Loading settings...</div>;
  }

  const choices = modelChoices(
    provider,
    installedModels,
    models,
    agent ? (agents.statuses[agent]?.models ?? []) : []
  );

  return (
    <>
      {error && <div className="alert alert-error">{error}</div>}
      {success && <div className="alert alert-success">{success}</div>}
      {notice && <div className="alert alert-warning">{notice}</div>}

      <form onSubmit={handleSave} className="settings-form">
        <div className="section-row">
          <div className="section-row-copy">
            <h2 className="section-title">AI Provider Configuration</h2>
            <p className="section-description">
              Defaults for every workspace in this organization; a workspace can override them.
            </p>
          </div>
        </div>

        <div className="settings-card">
          <h3 className="card-title">Provider</h3>
          <AiProviderFields
            level="organization"
            provider={provider}
            onProviderChange={(next) => {
              setProvider(next);
              setModels((previous) => ({ ...previous, fast: '', reasoning: '' }));
            }}
            credentials={credentials}
            configured={configured}
            onChange={(key, value) => setCredentials((previous) => ({ ...previous, [key]: value }))}
            saved={saved}
          />
          {agent && (
            <AgentSignIn
              key={`${currentOrganization.id}:${agent}`}
              organizationId={currentOrganization.id}
              agent={agent}
              access={agentAccess(currentOrganization.role, resolvingRole)}
              heading="h4"
              level="organization"
              unsaved={provider !== savedProvider}
              saveLabel={SAVE_LABEL}
              status={agents.statuses[agent]}
              attempt={agents.attempts[agent]}
              loadError={agents.error}
              onStatusChange={agents.update}
              onAttemptChange={agents.setAttempt}
            />
          )}
        </div>

        <div className="settings-card">
          <h3 className="card-title">Default Models</h3>
          <AiModelFields
            provider={provider}
            models={models}
            onChange={(key, value) => setModels((previous) => ({ ...previous, [key]: value }))}
            fastOptions={choices.fast}
            reasoningOptions={choices.reasoning}
            embeddingOptions={choices.embedding}
            installedModels={installedModels}
            inheritedLabel="Use server default"
          />
        </div>

        <div className="settings-actions">
          <Button type="button" variant="ghost" size="sm" onClick={handleReset} disabled={saving}>
            Reset to Defaults
          </Button>
          <Button type="submit" loading={saving}>
            {saving ? 'Saving...' : SAVE_LABEL}
          </Button>
        </div>
      </form>
    </>
  );
}
