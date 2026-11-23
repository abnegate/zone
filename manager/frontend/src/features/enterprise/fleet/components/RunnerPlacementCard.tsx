import { Button } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { community, lapsed, unverified } from '../../policy/license';
import { licenseDate } from '../../status';
import { useRunnerLabels, useUpdateRunnerLabels } from '../hooks';
import {
  CHATS_IGNORE_LABELS,
  DISCARD_LABELS,
  failureText,
  inheritedAlready,
  invalidLabels,
  LOADING_PLACEMENT,
  matchingText,
  noMatchWarning,
  ORGANIZATION_PLACEMENT_READ_ONLY,
  PLACEMENT_LAPSED,
  PLACEMENT_TITLE,
  PLACEMENT_UNVERIFIED,
  RETRY_LABEL,
  SAVE_LABELS,
  UPDATED_LABEL,
  WORKSPACE_PLACEMENT_READ_ONLY,
} from '../messages';
import {
  type Placement,
  PlacementKind,
  type PlacementScope,
  parseLabels,
  sameLabels,
  validLabel,
  withLabels,
  withoutLabel,
} from '../placement';
import { RunnerLabelsField } from './RunnerLabelsField';
import '../../policy/components/PolicySection.css';
import './RunnerPlacementCard.css';

interface RunnerPlacementCardProps {
  scope: PlacementScope;
}

interface Typed {
  labels: string[];
  problem: string | null;
}

function typedLabels(text: string, inherited: readonly string[]): Typed {
  const entries = parseLabels(text);
  const invalid = entries.filter((label) => !validLabel(label));
  if (invalid.length > 0) {
    return { labels: [], problem: invalidLabels(invalid) };
  }
  const repeated = entries.filter((label) => inherited.includes(label));
  if (repeated.length > 0) {
    return { labels: [], problem: inheritedAlready(repeated) };
  }
  return { labels: entries, problem: null };
}

interface PlacementEditorProps {
  scope: PlacementScope;
  placement: Placement;
}

function PlacementEditor({ scope, placement }: PlacementEditorProps) {
  const { update, pending, refusal, error, reset } = useUpdateRunnerLabels(scope);
  const [draft, setDraft] = useState<string[] | null>(null);
  const [text, setText] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const workspace = scope.kind === PlacementKind.Workspace;
  const labels = draft ?? placement.own;
  const dirty = !sameLabels(labels, placement.own) || text.trim() !== '';
  const effective = withLabels(placement.inherited, labels);
  const failure = problem ?? failureText(refusal, error);
  const readOnly = workspace ? WORKSPACE_PLACEMENT_READ_ONLY : ORGANIZATION_PLACEMENT_READ_ONLY;

  const change = (next: string[]) => {
    setDraft(next);
    setProblem(null);
    if (error) reset();
  };

  const add = (): string[] | null => {
    const typed = typedLabels(text, placement.inherited);
    if (typed.problem !== null) {
      setProblem(typed.problem);
      return null;
    }
    const next = withLabels(labels, typed.labels);
    change(next);
    setText('');
    return next;
  };

  const discard = () => {
    setDraft(null);
    setText('');
    setProblem(null);
    reset();
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending || !dirty || !placement.editable) {
      return;
    }
    const next = add();
    if (next === null) {
      return;
    }
    if (sameLabels(next, placement.own)) {
      setDraft(null);
      return;
    }
    update(next, () => setDraft(null));
  };

  return (
    <>
      {lapsed(placement) && <p className="policy-hint">{PLACEMENT_LAPSED}</p>}
      {unverified(placement) && <p className="policy-hint">{PLACEMENT_UNVERIFIED}</p>}
      {placement.entitled && !placement.editable && <p className="policy-hint">{readOnly}</p>}
      {placement.updated_at && (
        <p className="policy-hint">
          {UPDATED_LABEL} {licenseDate(placement.updated_at)}
        </p>
      )}
      <form className="policy-form" onSubmit={submit} noValidate>
        <fieldset className="settings-card policy-fields" disabled={pending}>
          <legend className="sr-only">{PLACEMENT_TITLE}</legend>
          <RunnerLabelsField
            workspace={workspace}
            labels={labels}
            inherited={placement.inherited}
            effective={effective}
            available={placement.available}
            editable={placement.editable}
            text={text}
            error={failure}
            onText={(next) => {
              setText(next);
              setProblem(null);
            }}
            onAdd={() => void add()}
            onPick={(label) => change(withLabels(labels, [label]))}
            onRemove={(label) => change(withoutLabel(labels, label))}
          />
          {placement.matching === 0 ? (
            <p className="policy-field-warning">{noMatchWarning(placement.effective)}</p>
          ) : (
            <p className="policy-field-note">{matchingText(placement.matching)}</p>
          )}
        </fieldset>
        {placement.editable && (
          <div className="policy-actions">
            <Button type="button" variant="ghost" disabled={!dirty || pending} onClick={discard}>
              {DISCARD_LABELS}
            </Button>
            <Button type="submit" loading={pending} disabled={!dirty}>
              {SAVE_LABELS}
            </Button>
          </div>
        )}
      </form>
    </>
  );
}

function scopeKey(scope: PlacementScope): string {
  return scope.kind === PlacementKind.Organization
    ? scope.organizationId
    : `${scope.organizationId}/${scope.workspaceId}`;
}

export function RunnerPlacementCard({ scope }: RunnerPlacementCardProps) {
  const headingId = useId();
  const { placement, loading, error, refresh } = useRunnerLabels(scope);

  if (placement !== undefined && community(placement) && !unverified(placement)) {
    return null;
  }

  return (
    <section className="runner-placement" aria-labelledby={headingId}>
      <div className="runner-placement-header">
        <h3 id={headingId} className="runner-placement-title">
          {PLACEMENT_TITLE}
        </h3>
        <p className="runner-placement-description">{CHATS_IGNORE_LABELS}</p>
      </div>
      {loading && <p className="policy-hint">{LOADING_PLACEMENT}</p>}
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            {RETRY_LABEL}
          </Button>
        </div>
      )}
      {placement && <PlacementEditor key={scopeKey(scope)} scope={scope} placement={placement} />}
    </section>
  );
}
