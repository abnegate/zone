import { Button, Checkbox, Input } from '@zone/ui';
import { type KeyboardEvent, useId, useState } from 'react';
import { allows, parseEntries, toggled, withEntries, withoutEntry } from '../draft';
import {
  fieldDescription,
  fieldLabel,
  NOT_ALLOWED,
  NOTHING_ALLOWED,
  unrestrictedText,
} from '../messages';
import { layerSource, PolicyScope } from '../source';
import type { PolicyListField as PolicyListFieldName } from '../types';
import { PolicyFieldFrame } from './PolicyFieldFrame';

export const RESTRICT_LABEL = 'Restrict';

export const OVERRIDE_LABEL = 'Override';

export const INHERIT_LABEL = 'Use organization policy';

export const ADD_LABEL = 'Add';

const SUBMIT_KEY = 'Enter';

const SUMMARY_SEPARATOR = ', ';

export interface PolicyOption {
  value: string;
  label: string;
}

interface PolicyListFieldProps {
  field: PolicyListFieldName;
  scope: PolicyScope;
  value: readonly string[] | null;
  organization?: readonly string[] | null;
  effective?: readonly string[] | null;
  options?: readonly PolicyOption[];
  placeholder?: string;
  editable: boolean;
  error?: string;
  onChange(value: string[] | null): void;
}

export function PolicyListField({
  field,
  scope,
  value,
  organization = null,
  effective = null,
  options,
  placeholder,
  editable,
  error,
  onChange,
}: PolicyListFieldProps) {
  const base = useId();
  const [text, setText] = useState('');
  const label = fieldLabel(field);
  const workspace = scope === PolicyScope.Workspace;
  const limit = workspace ? organization : null;

  const labelOf = (entry: string): string =>
    options?.find((option) => option.value === entry)?.label ?? entry;

  const summary = (list: readonly string[] | null): string => {
    if (list === null) {
      return unrestrictedText(field);
    }
    return list.length === 0 ? NOTHING_ALLOWED : list.map(labelOf).join(SUMMARY_SEPARATOR);
  };

  const start = (): string[] => {
    if (organization !== null) {
      return [...organization];
    }
    return options ? options.map((option) => option.value) : [];
  };

  const add = () => {
    const entries = parseEntries(text, field);
    if (value === null || entries.length === 0) {
      return;
    }
    onChange(withEntries(value, entries));
    setText('');
  };

  const submitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === SUBMIT_KEY) {
      event.preventDefault();
      add();
    }
  };

  const control = (() => {
    if (!editable) {
      return null;
    }
    if (workspace) {
      return value === null ? (
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`${OVERRIDE_LABEL} ${label}`}
          onClick={() => onChange(start())}
        >
          {OVERRIDE_LABEL}
        </Button>
      ) : (
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`${INHERIT_LABEL} for ${label}`}
          onClick={() => onChange(null)}
        >
          {INHERIT_LABEL}
        </Button>
      );
    }
    return (
      <Checkbox
        id={`${base}-restrict`}
        label={RESTRICT_LABEL}
        aria-label={`${RESTRICT_LABEL} ${label}`}
        checked={value !== null}
        onCheckedChange={(checked) => onChange(checked ? start() : null)}
      />
    );
  })();

  const checkboxes = (list: readonly string[], choices: readonly PolicyOption[]) => (
    <div className="policy-options">
      {choices.map((option) => {
        const checked = list.includes(option.value);
        const allowed = allows(limit, option.value);
        return (
          <div
            key={option.value}
            className={allowed ? 'policy-option' : 'policy-option policy-option--denied'}
          >
            <Checkbox
              id={`${base}-${option.value}`}
              label={option.label}
              helpText={allowed ? undefined : NOT_ALLOWED}
              checked={checked}
              disabled={!editable || (!allowed && !checked)}
              onCheckedChange={(on) => onChange(toggled(list, option.value, on))}
            />
          </div>
        );
      })}
    </div>
  );

  const chips = (list: readonly string[]) => (
    <>
      {list.length === 0 ? (
        <p className="policy-field-note">{NOTHING_ALLOWED}</p>
      ) : (
        <ul className="policy-chips" aria-label={label}>
          {list.map((entry) => (
            <li key={entry} className="policy-chip">
              <span className="policy-chip-text">{entry}</span>
              {editable && (
                <button
                  type="button"
                  className="policy-chip-remove"
                  aria-label={`Remove ${entry}`}
                  onClick={() => onChange(withoutEntry(list, entry))}
                >
                  <span aria-hidden="true">×</span>
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {editable && (
        <div className="policy-entry">
          <Input
            id={`${base}-entry`}
            aria-label={`Add to ${label}`}
            value={text}
            autoComplete="off"
            spellCheck={false}
            placeholder={placeholder}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={submitOnEnter}
          />
          <Button
            type="button"
            variant="secondary"
            size="sm"
            aria-label={`${ADD_LABEL} to ${label}`}
            disabled={text.trim() === ''}
            onClick={add}
          >
            {ADD_LABEL}
          </Button>
        </div>
      )}
    </>
  );

  const body = () => {
    if (value === null) {
      return workspace ? null : <p className="policy-field-note">{summary(null)}</p>;
    }
    return options ? checkboxes(value, options) : chips(value);
  };

  return (
    <PolicyFieldFrame
      label={label}
      description={fieldDescription(field)}
      source={workspace ? layerSource(value !== null, organization !== null) : undefined}
      organization={workspace ? summary(organization) : undefined}
      effective={workspace ? summary(effective) : undefined}
      control={control}
      error={error}
    >
      {body()}
    </PolicyFieldFrame>
  );
}
