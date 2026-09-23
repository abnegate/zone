import { Button, TabsContent, TabsList, TabsTrigger } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useAuth } from '../../../auth';
import { useModels } from '../../../models';
import {
  AgentSignIn,
  AiModelFields,
  AiProviderFields,
  agentOf,
  buildAiSettingsRequest,
  agentAccess,
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
import { SettingsPage } from '../../components';
import {
  AuditLogsSection,
  BillingSection,
  InvitationsSection,
  OrgMembersSection,
} from '../components';
import type { AiProvider, AiSettings, Workspace } from '../types';

type TabType = 'ai' | 'members' | 'invitations' | 'billing' | 'audit';

const TITLE = 'Organization Settings';

export default function OrgSettingsPage() {
  const { isAuthenticated } = useAuth();
  const { currentOrganization, resolvingRole } = useWorkspace();
  const { models: installedModels } = useModels();

  const [activeTab, setActiveTab] = useState<TabType>('ai');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);

  const [provider, setProvider] = useState<AiProvider>('self_hosted');
  const [savedProvider, setSavedProvider] = useState<AiProvider>('self_hosted');
  const [credentials, setCredentials] = useState<ProviderCredentials>(emptyCredentials);
  const [configured, setConfigured] = useState<ProviderConfigured>(nothingConfigured);
  const [models, setModels] = useState<ModelSelection>(emptyModels);

  const agent = agentOf(provider);
  const agents = useAgentStatuses(currentOrganization?.id ?? null, agent !== null);

  const applySettingsToForm = useCallback((settings: AiSettings) => {
    setProvider(settings.provider);
    setSavedProvider(settings.provider);
    setCredentials(credentialsFromSettings(settings));
    setConfigured(configuredFromSettings(settings));
    setModels(modelsFromSettings(settings));
  }, []);

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
  }, [isAuthenticated, currentOrganization, applySettingsToForm]);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  const flash = (message: string) => {
    setSuccess(message);
    setTimeout(() => setSuccess(null), 3000);
  };

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !currentOrganization) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      const settings = await client.updateOrgAiSettings(
        currentOrganization.id,
        buildAiSettingsRequest(provider, credentials, models)
      );
      applySettingsToForm(settings);
      flash('Settings saved successfully');
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
      flash('Settings reset to defaults');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to reset settings');
    } finally {
      setSaving(false);
    }
  };

  const choices = modelChoices(
    provider,
    installedModels,
    models,
    agent ? (agents.statuses[agent]?.models ?? []) : []
  );

  const tabs = (
    <TabsList aria-label="Organization settings">
      <TabsTrigger value="ai">AI Settings</TabsTrigger>
      <TabsTrigger value="members">Members</TabsTrigger>
      <TabsTrigger value="invitations">Invitations</TabsTrigger>
      <TabsTrigger value="billing">Billing</TabsTrigger>
      <TabsTrigger value="audit">Audit Logs</TabsTrigger>
    </TabsList>
  );

  if (!currentOrganization) {
    return (
      <SettingsPage title={TITLE}>
        <div className="loading-state">Please select an organization</div>
      </SettingsPage>
    );
  }

  const selectTab = (value: string) => setActiveTab(value as TabType);

  if (loading && activeTab === 'ai') {
    return (
      <SettingsPage title={TITLE} tabs={tabs} value={activeTab} onValueChange={selectTab}>
        <div className="loading-state">Loading settings...</div>
      </SettingsPage>
    );
  }

  return (
    <SettingsPage title={TITLE} tabs={tabs} value={activeTab} onValueChange={selectTab}>
      {error && <div className="alert alert-error">{error}</div>}
      {success && <div className="alert alert-success">{success}</div>}

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
              provider={provider}
              onProviderChange={(next) => {
                setProvider(next);
                setModels((previous) => ({ ...previous, fast: '', reasoning: '' }));
              }}
              credentials={credentials}
              configured={configured}
              onChange={(key, value) => setCredentials((prev) => ({ ...prev, [key]: value }))}
            />
            {agent && (
              <AgentSignIn
                key={`${currentOrganization.id}:${agent}`}
                organizationId={currentOrganization.id}
                agent={agent}
                access={agentAccess(currentOrganization.role, resolvingRole)}
                unsaved={provider !== savedProvider}
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
              onChange={(key, value) => setModels((prev) => ({ ...prev, [key]: value }))}
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
              {saving ? 'Saving...' : 'Save Changes'}
            </Button>
          </div>
        </form>
      </TabsContent>
    </SettingsPage>
  );
}
