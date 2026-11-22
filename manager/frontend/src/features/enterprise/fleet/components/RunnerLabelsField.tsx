import { Button, Input } from '@zone/ui';
import { type KeyboardEvent, useId } from 'react';
import { PolicyFieldFrame } from '../../policy/components/PolicyFieldFrame';
import { layerSource } from '../../policy/source';
import {
  ADD_LABEL,
  AVAILABLE_LABEL,
  EFFECTIVE_LABEL,
  ENTRY_LABEL,
  ENTRY_PLACEHOLDER,
  INHERITED_NOTE,
  NO_AVAILABLE,
  NOTHING_REQUIRED,
  ORGANIZATION_REQUIRED_LABEL,
  PLACEMENT_DESCRIPTION,
  pendingLabels,
  REQUIRED_LABEL,
  removeLabel,
  requireLabel,
  runnerCount,
  WORKSPACE_REQUIRED_LABEL,
} from '../messages';
import { parseLabels } from '../placement';
import type { AvailableLabel } from '../types';

const SUBMIT_KEY = 'Enter';

const SUMMARY_SEPARATOR = ', ';

interface RunnerLabelsFieldProps {
  workspace: boolean;
  labels: readonly string[];
  inherited: readonly string[];
  effective: readonly string[];
  available: readonly AvailableLabel[];
  editable: boolean;
  text: string;
  error: string | null;
  onText(text: string): void;
  onAdd(): void;
  onPick(label: string): void;
  onRemove(label: string): void;
}

function summary(labels: readonly string[]): string {
  return labels.length === 0 ? NOTHING_REQUIRED : labels.join(SUMMARY_SEPARATOR);
}

export function RunnerLabelsField({
  workspace,
  labels,
  inherited,
  effective,
  available,
  editable,
  text,
  error,
  onText,
  onAdd,
  onPick,
  onRemove,
}: RunnerLabelsFieldProps) {
  const base = useId();
  const errorId = `${base}-error`;
  const pendingId = `${base}-pending`;
  const pending = parseLabels(text);
  const announced = editable && pending.length > 0 && !error;
  const required = new Set([...inherited, ...labels]);
  const offered = available.filter((entry) => !required.has(entry.label));
  const ownLabel = workspace ? WORKSPACE_REQUIRED_LABEL : REQUIRED_LABEL;

  const submitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === SUBMIT_KEY) {
      event.preventDefault();
      onAdd();
    }
  };

  const described = [error ? errorId : null, announced ? pendingId : null]
    .filter((id) => id !== null)
    .join(' ');

  return (
    <PolicyFieldFrame
      label={REQUIRED_LABEL}
      description={PLACEMENT_DESCRIPTION}
      source={workspace ? layerSource(labels.length > 0, inherited.length > 0) : undefined}
      error={error ?? undefined}
      errorId={errorId}
    >
      {workspace && (
        <div className="runner-placement-layer">
          <p className="runner-placement-layer-label">{ORGANIZATION_REQUIRED_LABEL}</p>
          {inherited.length === 0 ? (
            <p className="policy-field-note">{NOTHING_REQUIRED}</p>
          ) : (
            <>
              <ul className="policy-chips" aria-label={ORGANIZATION_REQUIRED_LABEL}>
                {inherited.map((label) => (
                  <li key={label} className="policy-chip runner-placement-chip--locked">
                    <span className="policy-chip-text">{label}</span>
                  </li>
                ))}
              </ul>
              <p className="policy-field-note">{INHERITED_NOTE}</p>
            </>
          )}
        </div>
      )}
      <div className="runner-placement-layer">
        {workspace && <p className="runner-placement-layer-label">{ownLabel}</p>}
        {labels.length === 0 ? (
          <p className="policy-field-note">{NOTHING_REQUIRED}</p>
        ) : (
          <ul className="policy-chips" aria-label={ownLabel}>
            {labels.map((label) => (
              <li key={label} className="policy-chip">
                <span className="policy-chip-text">{label}</span>
                {editable && (
                  <button
                    type="button"
                    className="policy-chip-remove"
                    aria-label={removeLabel(label)}
                    onClick={() => onRemove(label)}
                  >
                    <span aria-hidden="true">×</span>
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      {workspace && (
        <p className="policy-field-summary">
          <span className="policy-field-summary-label">{EFFECTIVE_LABEL}</span>
          <span className="policy-field-summary-value">{summary(effective)}</span>
        </p>
      )}
      {editable && (
        <>
          <div className="policy-entry">
            <Input
              id={`${base}-entry`}
              aria-label={ENTRY_LABEL}
              aria-invalid={error ? true : undefined}
              aria-describedby={described === '' ? undefined : described}
              value={text}
              autoComplete="off"
              spellCheck={false}
              placeholder={ENTRY_PLACEHOLDER}
              onChange={(event) => onText(event.target.value)}
              onKeyDown={submitOnEnter}
            />
            <Button
              type="button"
              variant="secondary"
              size="sm"
              aria-label={ENTRY_LABEL}
              disabled={pending.length === 0}
              onClick={onAdd}
            >
              {ADD_LABEL}
            </Button>
          </div>
          {announced && (
            <p id={pendingId} className="policy-field-note">
              {pendingLabels(pending)}
            </p>
          )}
          <div className="runner-placement-available">
            <p className="runner-placement-layer-label">{AVAILABLE_LABEL}</p>
            {available.length === 0 ? (
              <p className="policy-field-note">{NO_AVAILABLE}</p>
            ) : (
              offered.length > 0 && (
                <ul className="runner-placement-options" aria-label={AVAILABLE_LABEL}>
                  {offered.map((entry) => (
                    <li key={entry.label}>
                      <button
                        type="button"
                        className="runner-placement-option"
                        aria-label={requireLabel(entry.label)}
                        onClick={() => onPick(entry.label)}
                      >
                        <span className="runner-placement-option-label">{entry.label}</span>
                        <span className="runner-placement-option-count">
                          {runnerCount(entry.runners)}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )
            )}
          </div>
        </>
      )}
    </PolicyFieldFrame>
  );
}
