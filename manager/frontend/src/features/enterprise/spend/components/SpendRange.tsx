import { Button, Input, Select, type SelectOption } from '@zone/ui';
import { useId, useState } from 'react';
import { type SpendTarget, useSpendExport } from '../hooks';
import {
  ALL_WORKSPACES,
  DOWNLOAD_LABEL,
  DOWNLOADED_LABEL,
  EVERYONE,
  FROM_LABEL,
  PERSON_FILTER_LABEL,
  RANGE_LEGEND,
  TO_LABEL,
  WORKSPACE_FILTER_LABEL,
} from '../labels';
import { refusalMessage } from '../messages';
import { validRange } from '../range';
import { SpendCode, type SpendQuery } from '../schemas';

const ALL = '';

function options(all: string, targets: readonly SpendTarget[]): SelectOption[] {
  return [
    { value: ALL, label: all },
    ...targets.map((target) => ({ value: target.id, label: target.label })),
  ];
}

interface SpendRangeProps {
  organizationId: string;
  query: SpendQuery;
  workspaces: readonly SpendTarget[];
  people: readonly SpendTarget[];
  onChange(query: SpendQuery): void;
}

export function SpendRange({
  organizationId,
  query,
  workspaces,
  people,
  onChange,
}: SpendRangeProps) {
  const base = useId();
  const fromId = `${base}-from`;
  const [from, setFrom] = useState(query.from);
  const [to, setTo] = useState(query.to);
  const [downloaded, setDownloaded] = useState<string | null>(null);
  const exporter = useSpendExport(organizationId);
  const valid = validRange({ from, to });
  const rangeError = valid ? undefined : refusalMessage(SpendCode.RangeInvalid);
  const failure = exporter.refusal ? refusalMessage(exporter.refusal.code) : exporter.error;

  const touch = () => {
    setDownloaded(null);
    exporter.reset();
  };

  const dates = (next: { from: string; to: string }) => {
    setFrom(next.from);
    setTo(next.to);
    touch();
    if (validRange(next)) {
      onChange({ ...query, ...next });
    }
  };

  const filter = (key: 'workspace_id' | 'user_id', value: string) => {
    touch();
    onChange({ ...query, [key]: value === ALL ? undefined : value });
  };

  return (
    <div className="settings-card spend-range">
      <fieldset className="spend-range-fields">
        <legend className="sr-only">{RANGE_LEGEND}</legend>
        <Input
          id={fromId}
          label={FROM_LABEL}
          type="date"
          value={from}
          max={to || undefined}
          error={rangeError}
          onChange={(event) => dates({ from: event.target.value, to })}
        />
        <Input
          id={`${base}-to`}
          label={TO_LABEL}
          type="date"
          value={to}
          min={from || undefined}
          aria-invalid={rangeError !== undefined}
          aria-describedby={rangeError ? `${fromId}-error` : undefined}
          onChange={(event) => dates({ from, to: event.target.value })}
        />
        <Select
          id={`${base}-workspace`}
          label={WORKSPACE_FILTER_LABEL}
          value={query.workspace_id ?? ALL}
          options={options(ALL_WORKSPACES, workspaces)}
          onValueChange={(value) => filter('workspace_id', value)}
        />
        <Select
          id={`${base}-person`}
          label={PERSON_FILTER_LABEL}
          value={query.user_id ?? ALL}
          options={options(EVERYONE, people)}
          onValueChange={(value) => filter('user_id', value)}
        />
      </fieldset>
      {failure && (
        <div className="alert alert-error" role="alert">
          {failure}
        </div>
      )}
      <div className="spend-actions">
        {downloaded && (
          <span className="spend-saved" role="status">
            {DOWNLOADED_LABEL} {downloaded}
          </span>
        )}
        <Button
          type="button"
          size="sm"
          variant="outline"
          loading={exporter.pending}
          disabled={!valid}
          onClick={() => exporter.download(query, (download) => setDownloaded(download.filename))}
        >
          {DOWNLOAD_LABEL}
        </Button>
      </div>
    </div>
  );
}
