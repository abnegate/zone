import { Button } from '@zone/ui';
import { type FormEvent, useState } from 'react';
import {
  changed,
  ENTRY_FIELDS,
  type PolicyEntryField,
  type PolicyErrors,
  type PolicyTexts,
  parseEntries,
  patchFrom,
  typed,
  uncovered,
  withEntries,
} from '../draft';
import {
  agentLabel,
  isAdminCode,
  notAllowed,
  providerLabel,
  refusalField,
  refusalMessage,
} from '../messages';
import type { PolicyRefusal } from '../PolicyRefusal';
import {
  POLICY_AGENTS,
  POLICY_PROVIDERS,
  PolicyAgentSchema,
  PolicyCode,
  PolicyField,
  PolicyProviderSchema,
} from '../schemas';
import { PolicyScope } from '../source';
import type { PolicyAgent, PolicyProvider, PolicyRules, UpdatePolicyRequest } from '../types';
import { PolicyCeilingField } from './PolicyCeilingField';
import { PolicyListField, type PolicyOption } from './PolicyListField';

export const SAVE_LABEL = 'Save policy';

export const DISCARD_LABEL = 'Discard changes';

export const LEGENDS: Readonly<Record<PolicyScope, string>> = {
  [PolicyScope.Organization]: 'Organization agent policy',
  [PolicyScope.Workspace]: 'Workspace agent policy',
};

const PROVIDER_OPTIONS: readonly PolicyOption[] = POLICY_PROVIDERS.map((provider) => ({
  value: provider,
  label: providerLabel(provider),
}));

const AGENT_OPTIONS: readonly PolicyOption[] = POLICY_AGENTS.map((agent) => ({
  value: agent,
  label: agentLabel(agent),
}));

const PLACEHOLDERS: Readonly<Record<PolicyEntryField, string>> = {
  [PolicyField.Models]: 'claude-sonnet-4, gpt-5*',
  [PolicyField.McpServers]: 'github, linear',
  [PolicyField.Domains]: 'github.com, example.com',
};

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

function without<T extends object>(record: T, key: keyof T): T {
  const { [key]: _, ...rest } = record;
  return rest as T;
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
  if (refusal.code === PolicyCode.Wider) {
    return { fields: {}, form: refusal.message.trim() || refusalMessage(refusal.code) };
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
  const [edits, setEdits] = useState<Partial<PolicyRules>>({});
  const [texts, setTexts] = useState<PolicyTexts>({});
  const [rejections, setRejections] = useState<PolicyErrors>({});
  const current: PolicyRules = { ...saved, ...edits };
  const dirty = changed(patchFrom(saved, current)) || typed(texts);
  const problems = problemsOf(refusal, error);
  const errors: PolicyErrors = { ...problems.fields, ...rejections };
  const workspace = scope === PolicyScope.Workspace;

  const change = (update: Partial<PolicyRules>) => {
    setEdits((previous) => ({ ...previous, ...update }));
    if (error) {
      onReset();
    }
  };

  const write = (field: PolicyEntryField, text: string) => {
    setTexts((previous) => ({ ...previous, [field]: text }));
    setRejections((previous) => without(previous, field));
  };

  const rejection = (field: PolicyEntryField, entries: readonly string[]): string | null => {
    if (!workspace) {
      return null;
    }
    const outside = uncovered(field, organization?.[field] ?? null, entries);
    return outside.length === 0 ? null : notAllowed(outside);
  };

  const add = (field: PolicyEntryField) => {
    const list = current[field];
    const entries = parseEntries(texts[field] ?? '', field);
    if (list === null || entries.length === 0) {
      return;
    }
    const refused = rejection(field, entries);
    if (refused !== null) {
      setRejections((previous) => ({ ...previous, [field]: refused }));
      return;
    }
    change({ [field]: withEntries(list, entries) });
    write(field, '');
  };

  const discard = () => {
    setEdits({});
    setTexts({});
    setRejections({});
    onReset();
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending || !dirty || !editable) {
      return;
    }
    const rules: PolicyRules = { ...current };
    const refused: PolicyErrors = {};
    for (const field of ENTRY_FIELDS) {
      const list = rules[field];
      const entries = parseEntries(texts[field] ?? '', field);
      if (list === null || entries.length === 0) {
        continue;
      }
      const reason = rejection(field, entries);
      if (reason === null) {
        rules[field] = withEntries(list, entries);
      } else {
        refused[field] = reason;
      }
    }
    if (Object.keys(refused).length > 0) {
      setRejections(refused);
      return;
    }
    const request = patchFrom(saved, rules);
    const settle = () => {
      setEdits({});
      setTexts({});
    };
    if (!changed(request)) {
      settle();
      return;
    }
    onSave(request, settle);
  };

  const shared = { scope, editable };

  const entryField = (field: PolicyEntryField) => (
    <PolicyListField
      {...shared}
      field={field}
      value={current[field]}
      organization={organization?.[field]}
      effective={effective?.[field]}
      placeholder={PLACEHOLDERS[field]}
      error={errors[field]}
      text={texts[field] ?? ''}
      onText={(text) => write(field, text)}
      onAdd={() => add(field)}
      onChange={(value) => {
        change({ [field]: value });
        if (value === null) {
          write(field, '');
        }
      }}
    />
  );

  return (
    <form className="policy-form" onSubmit={submit} noValidate>
      <fieldset className="settings-card policy-fields" disabled={pending}>
        <legend className="sr-only">{LEGENDS[scope]}</legend>
        <PolicyListField
          {...shared}
          field={PolicyField.Providers}
          value={current.providers}
          organization={organization?.providers}
          effective={effective?.providers}
          options={PROVIDER_OPTIONS}
          error={errors[PolicyField.Providers]}
          onChange={(value) => change({ providers: providersOf(value) })}
        />
        <PolicyListField
          {...shared}
          field={PolicyField.Agents}
          value={current.agents}
          organization={organization?.agents}
          effective={effective?.agents}
          options={AGENT_OPTIONS}
          error={errors[PolicyField.Agents]}
          onChange={(value) => change({ agents: agentsOf(value) })}
        />
        {entryField(PolicyField.Models)}
        <PolicyCeilingField
          {...shared}
          value={current.ceiling}
          organization={organization?.ceiling}
          effective={effective?.ceiling}
          error={errors[PolicyField.Ceiling]}
          onChange={(value) => change({ ceiling: value })}
        />
        {entryField(PolicyField.McpServers)}
        {entryField(PolicyField.Domains)}
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
