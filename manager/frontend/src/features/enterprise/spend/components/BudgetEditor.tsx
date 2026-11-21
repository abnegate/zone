import { Button, Checkbox, Input, Select } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import {
  type BudgetDraft,
  type BudgetErrors,
  createRequest,
  draftOf,
  EMPTY_DRAFT,
  errorsOf,
  updateRequest,
} from '../draft';
import type { BudgetMutations, MutationState, SpendTarget } from '../hooks';
import {
  ADD_LABEL,
  ADD_TITLE,
  CANCEL_LABEL,
  CHOOSE_PERSON,
  CHOOSE_WORKSPACE,
  EDIT_TITLE,
  HARD_LABEL,
  HARD_LIMIT_NOTE,
  LIMIT_LABEL,
  LIMIT_PLACEHOLDER,
  PERSON_PICKER_LABEL,
  SAVE_LABEL,
  SCOPE_LABEL,
  THRESHOLD_HELP,
  THRESHOLD_LABEL,
  WORKSPACE_PICKER_LABEL,
} from '../labels';
import { BudgetField, budgetTarget, refusalField, refusalMessage, scopeLabel } from '../messages';
import {
  BUDGET_SCOPES,
  type Budget,
  BudgetScope,
  type BudgetScope as BudgetScopeValue,
  THRESHOLD_MAXIMUM,
  THRESHOLD_MINIMUM,
} from '../schemas';

const SCOPE_OPTIONS = BUDGET_SCOPES.map((scope) => ({ value: scope, label: scopeLabel(scope) }));

function targetOptions(targets: readonly SpendTarget[]) {
  return targets.map((target) => ({ value: target.id, label: target.label }));
}

function isScope(value: string): value is BudgetScopeValue {
  return (BUDGET_SCOPES as readonly string[]).includes(value);
}

interface Problems {
  fields: BudgetErrors;
  form: string | null;
}

function problemsOf(mutation: MutationState, shown: (field: BudgetField) => boolean): Problems {
  if (mutation.refusal) {
    const field = refusalField(mutation.refusal.code);
    const message = refusalMessage(mutation.refusal.code);
    return field && shown(field)
      ? { fields: { [field]: message }, form: null }
      : { fields: {}, form: message };
  }
  return { fields: {}, form: mutation.error };
}

interface BudgetEditorProps {
  budget: Budget | null;
  editable: boolean;
  workspaces: readonly SpendTarget[];
  people: readonly SpendTarget[];
  mutations: Pick<BudgetMutations, 'create' | 'update'>;
  onDone(): void;
}

export function BudgetEditor({
  budget,
  editable,
  workspaces,
  people,
  mutations,
  onDone,
}: BudgetEditorProps) {
  const base = useId();
  const headingId = `${base}-heading`;
  const [draft, setDraft] = useState<BudgetDraft>(() => (budget ? draftOf(budget) : EMPTY_DRAFT));
  const [attempted, setAttempted] = useState(false);
  const mutation = budget ? mutations.update : mutations.create;
  const locked = !editable || mutation.pending;
  const picksTarget = budget === null && draft.scope !== BudgetScope.Organization;
  const server = problemsOf(mutation, (field) => field !== BudgetField.Target || picksTarget);
  const errors: BudgetErrors = { ...(attempted ? errorsOf(draft) : {}), ...server.fields };

  const change = (next: Partial<BudgetDraft>) => {
    setDraft((current) => ({ ...current, ...next }));
    if (mutation.error) mutation.reset();
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setAttempted(true);
    if (locked) return;
    if (budget) {
      const request = updateRequest(budget, draft);
      if (request === null) return;
      if (Object.keys(request).length === 0) {
        onDone();
        return;
      }
      mutations.update.run(budget.id, request, onDone);
      return;
    }
    const request = createRequest(draft);
    if (request === null) return;
    mutations.create.run(request, () => {
      setDraft(EMPTY_DRAFT);
      setAttempted(false);
      onDone();
    });
  };

  return (
    <form
      className="settings-card spend-editor"
      aria-labelledby={headingId}
      onSubmit={submit}
      noValidate
    >
      <h3 id={headingId} className="spend-editor-title">
        {budget ? EDIT_TITLE : ADD_TITLE}
      </h3>
      <fieldset className="spend-editor-fields" disabled={locked}>
        {budget ? (
          <p className="spend-editor-target">
            {scopeLabel(budget.scope)}: <strong>{budgetTarget(budget)}</strong>
          </p>
        ) : (
          <div className="spend-editor-target-fields">
            <Select
              id={`${base}-scope`}
              label={SCOPE_LABEL}
              value={draft.scope}
              options={SCOPE_OPTIONS}
              disabled={locked}
              onValueChange={(value) => {
                if (isScope(value)) change({ scope: value });
              }}
            />
            {draft.scope === BudgetScope.Workspace && (
              <Select
                id={`${base}-workspace`}
                label={WORKSPACE_PICKER_LABEL}
                value={draft.workspace || undefined}
                placeholder={CHOOSE_WORKSPACE}
                options={targetOptions(workspaces)}
                disabled={locked}
                error={errors[BudgetField.Target]}
                onValueChange={(value) => change({ workspace: value })}
              />
            )}
            {draft.scope === BudgetScope.User && (
              <Select
                id={`${base}-person`}
                label={PERSON_PICKER_LABEL}
                value={draft.person || undefined}
                placeholder={CHOOSE_PERSON}
                options={targetOptions(people)}
                disabled={locked}
                error={errors[BudgetField.Target]}
                onValueChange={(value) => change({ person: value })}
              />
            )}
          </div>
        )}
        <div className="spend-editor-amounts">
          <Input
            id={`${base}-limit`}
            label={LIMIT_LABEL}
            inputMode="decimal"
            autoComplete="off"
            placeholder={LIMIT_PLACEHOLDER}
            value={draft.limit}
            disabled={locked}
            error={errors[BudgetField.Limit]}
            onChange={(event) => change({ limit: event.target.value })}
          />
          <Input
            id={`${base}-threshold`}
            label={THRESHOLD_LABEL}
            type="number"
            inputMode="numeric"
            min={THRESHOLD_MINIMUM}
            max={THRESHOLD_MAXIMUM}
            step={1}
            value={draft.threshold}
            disabled={locked}
            helpText={THRESHOLD_HELP}
            error={errors[BudgetField.Threshold]}
            onChange={(event) => change({ threshold: event.target.value })}
          />
        </div>
        <Checkbox
          id={`${base}-hard`}
          label={HARD_LABEL}
          helpText={HARD_LIMIT_NOTE}
          checked={draft.hard}
          disabled={locked}
          onCheckedChange={(hard) => change({ hard })}
        />
      </fieldset>
      {server.form && (
        <div className="alert alert-error" role="alert">
          {server.form}
        </div>
      )}
      {editable && (
        <div className="spend-actions">
          {budget && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={mutation.pending}
              onClick={() => {
                mutation.reset();
                onDone();
              }}
            >
              {CANCEL_LABEL}
            </Button>
          )}
          <Button type="submit" size="sm" loading={mutation.pending}>
            {budget ? SAVE_LABEL : ADD_LABEL}
          </Button>
        </div>
      )}
    </form>
  );
}
