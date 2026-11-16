import { Button } from '@zone/ui';
import { type FormEvent, useState } from 'react';
import { changed, type PolicyErrors, patchFrom } from '../draft';
import { agentLabel, isAdminCode, providerLabel, refusalField, refusalMessage } from '../messages';
import type { PolicyRefusal } from '../PolicyRefusal';
import {
  POLICY_AGENTS,
  POLICY_PROVIDERS,
  PolicyAgentSchema,
  PolicyField,
  PolicyProviderSchema,
} from '../schemas';
import type { PolicyScope } from '../source';
import type { PolicyAgent, PolicyProvider, PolicyRules, UpdatePolicyRequest } from '../types';
import { PolicyCeilingField } from './PolicyCeilingField';
import { PolicyListField, type PolicyOption } from './PolicyListField';

export const SAVE_LABEL = 'Save policy';

export const DISCARD_LABEL = 'Discard changes';

const PROVIDER_OPTIONS: readonly PolicyOption[] = POLICY_PROVIDERS.map((provider) => ({
  value: provider,
  label: providerLabel(provider),
}));

const AGENT_OPTIONS: readonly PolicyOption[] = POLICY_AGENTS.map((agent) => ({
  value: agent,
  label: agentLabel(agent),
}));

const PLACEHOLDERS = {
  models: 'claude-sonnet-4, gpt-5*',
  mcpServers: 'github, linear',
  domains: 'github.com, example.com',
} as const;

function isProvider(value: string): value is PolicyProvider {
  return PolicyProviderSchema.safeParse(value).success;
}

function isAgent(value: string): value is PolicyAgent {
  return PolicyAgentSchema.safeParse(value).success;
}

function providersOf(list: string[] | null): PolicyProvider[] | null {
  return list === null ? null : list.filter(isProvider);
}

function agentsOf(list: string[] | null): PolicyAgent[] | null {
  return list === null ? null : list.filter(isAgent);
}

interface Problems {
  fields: PolicyErrors;
  form: string | null;
}

function problemsOf(refusal: PolicyRefusal | null, error: string | null): Problems {
  if (refusal === null) {
    return { fields: {}, form: error };
  }
  if (!isAdminCode(refusal.code)) {
    return { fields: {}, form: refusal.message };
  }
  const field = refusalField(refusal.code);
  const message = refusalMessage(refusal.code);
  return field === null
    ? { fields: {}, form: message }
    : { fields: { [field]: message }, form: null };
}

interface PolicyEditorProps {
  scope: PolicyScope;
  saved: PolicyRules;
  organization?: PolicyRules;
  effective?: PolicyRules;
  editable: boolean;
  pending: boolean;
  refusal: PolicyRefusal | null;
  error: string | null;
  onSave(request: UpdatePolicyRequest, onSaved: () => void): void;
  onReset(): void;
}

export function PolicyEditor({
  scope,
  saved,
  organization,
  effective,
  editable,
  pending,
  refusal,
  error,
  onSave,
  onReset,
}: PolicyEditorProps) {
  const [draft, setDraft] = useState<PolicyRules | null>(null);
  const current = draft ?? saved;
  const request = patchFrom(saved, current);
  const dirty = changed(request);
  const problems = problemsOf(refusal, error);

  const change = (update: Partial<PolicyRules>) => {
    setDraft({ ...current, ...update });
    if (error) {
      onReset();
    }
  };

  const discard = () => {
    setDraft(null);
    onReset();
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending || !dirty || !editable) {
      return;
    }
    onSave(request, () => setDraft(null));
  };

  const shared = { scope, editable };

  return (
    <form className="policy-form" onSubmit={submit} noValidate>
      <fieldset className="settings-card policy-fields" disabled={pending}>
        <PolicyListField
          {...shared}
          field={PolicyField.Providers}
          value={current.providers}
          organization={organization?.providers}
          effective={effective?.providers}
          options={PROVIDER_OPTIONS}
          error={problems.fields[PolicyField.Providers]}
          onChange={(value) => change({ providers: providersOf(value) })}
        />
        <PolicyListField
          {...shared}
          field={PolicyField.Agents}
          value={current.agents}
          organization={organization?.agents}
          effective={effective?.agents}
          options={AGENT_OPTIONS}
          error={problems.fields[PolicyField.Agents]}
          onChange={(value) => change({ agents: agentsOf(value) })}
        />
        <PolicyListField
          {...shared}
          field={PolicyField.Models}
          value={current.models}
          organization={organization?.models}
          effective={effective?.models}
          placeholder={PLACEHOLDERS.models}
          error={problems.fields[PolicyField.Models]}
          onChange={(value) => change({ models: value })}
        />
        <PolicyCeilingField
          {...shared}
          value={current.ceiling}
          organization={organization?.ceiling}
          effective={effective?.ceiling}
          error={problems.fields[PolicyField.Ceiling]}
          onChange={(value) => change({ ceiling: value })}
        />
        <PolicyListField
          {...shared}
          field={PolicyField.McpServers}
          value={current.mcp_servers}
          organization={organization?.mcp_servers}
          effective={effective?.mcp_servers}
          placeholder={PLACEHOLDERS.mcpServers}
          error={problems.fields[PolicyField.McpServers]}
          onChange={(value) => change({ mcp_servers: value })}
        />
        <PolicyListField
          {...shared}
          field={PolicyField.Domains}
          value={current.domains}
          organization={organization?.domains}
          effective={effective?.domains}
          placeholder={PLACEHOLDERS.domains}
          error={problems.fields[PolicyField.Domains]}
          onChange={(value) => change({ domains: value })}
        />
      </fieldset>
      {problems.form && (
        <div className="alert alert-error" role="alert">
          {problems.form}
        </div>
      )}
      {editable && (
        <div className="policy-actions">
          <Button type="button" variant="ghost" disabled={!dirty || pending} onClick={discard}>
            {DISCARD_LABEL}
          </Button>
          <Button type="submit" loading={pending} disabled={!dirty}>
            {SAVE_LABEL}
          </Button>
        </div>
      )}
    </form>
  );
}
