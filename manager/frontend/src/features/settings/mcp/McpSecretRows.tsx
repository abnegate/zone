import { Button, Input } from '@zone/ui';
import { useId, useRef, useState } from 'react';
import {
  type DraftErrors,
  DraftField,
  emptyRow,
  RowPart,
  renamedRow,
  rowField,
  type SecretDraft,
  type SecretList,
} from './draft';

export const SAVED_PLACEHOLDER = 'Saved';

interface SecretLabels {
  legend: string;
  name: string;
  noun: string;
  add: string;
}

const LABELS: Readonly<Record<SecretList, SecretLabels>> = {
  [DraftField.Environment]: {
    legend: 'Environment variables',
    name: 'Variable name',
    noun: 'variable',
    add: 'Add variable',
  },
  [DraftField.Headers]: {
    legend: 'Headers',
    name: 'Header name',
    noun: 'header',
    add: 'Add header',
  },
};

interface McpSecretRowsProps {
  list: SecretList;
  rows: readonly SecretDraft[];
  errors: DraftErrors;
  onChange: (rows: SecretDraft[]) => void;
}

export function McpSecretRows({ list, rows, errors, onChange }: McpSecretRowsProps) {
  const base = useId();
  const counter = useRef(rows.length);
  const [keys, setKeys] = useState<number[]>(() => rows.map((_, index) => index));
  const labels = LABELS[list];

  const replace = (index: number, row: SecretDraft) =>
    onChange(rows.map((current, position) => (position === index ? row : current)));

  const add = () => {
    setKeys((current) => [...current, counter.current++]);
    onChange([...rows, emptyRow()]);
  };

  const remove = (index: number) => {
    setKeys((current) => current.filter((_, position) => position !== index));
    onChange(rows.filter((_, position) => position !== index));
  };

  return (
    <fieldset className="mcp-secrets">
      <legend className="form-label">{labels.legend}</legend>
      {rows.length > 0 && (
        <ul className="mcp-secret-list">
          {rows.map((row, index) => {
            const number = index + 1;
            const key = keys[index] ?? index;
            const label = row.name || `${labels.noun} ${number}`;
            return (
              <li key={key} className="mcp-secret-row">
                <Input
                  id={`${base}-${key}-name`}
                  aria-label={`${labels.name} ${number}`}
                  value={row.name}
                  autoComplete="off"
                  spellCheck={false}
                  error={errors[rowField(list, index, RowPart.Name)]}
                  onChange={(event) => replace(index, renamedRow(row, event.target.value))}
                />
                <Input
                  id={`${base}-${key}-value`}
                  type="password"
                  autoComplete="new-password"
                  aria-label={`Value for ${row.name || number}`}
                  value={row.value}
                  placeholder={row.saved && row.value === '' ? SAVED_PLACEHOLDER : undefined}
                  error={errors[rowField(list, index, RowPart.Value)]}
                  onChange={(event) => replace(index, { ...row, value: event.target.value })}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={`Remove ${label}`}
                  onClick={() => remove(index)}
                >
                  Remove
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      <Button variant="outline" size="sm" onClick={add}>
        {labels.add}
      </Button>
    </fieldset>
  );
}
