import { Button, Input, Select } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { useAuditExport } from '../hooks';
import {
  DOWNLOAD_DESCRIPTION,
  DOWNLOAD_LABEL,
  DOWNLOAD_MEMORY_NOTE,
  DOWNLOAD_TITLE,
  DOWNLOADED_LABEL,
  END_DATE_LABEL,
  FORMAT_LABEL,
  START_DATE_LABEL,
} from '../labels';
import { formatLabel, LicensedFeature, readOnlyMessage, refusalMessage } from '../messages';
import { AuditCode, EXPORT_FORMATS, ExportFormat } from '../schemas';
import type { AuditScope, ExportFormat as ExportFormatValue, Standing } from '../types';

const DEFAULT_RANGE_DAYS = 30;

const DAY_MILLISECONDS = 86_400_000;

const DATE_LENGTH = 10;

const FORMAT_OPTIONS = EXPORT_FORMATS.map((format) => ({
  value: format,
  label: formatLabel(format),
}));

function isoDate(time: number): string {
  return new Date(time).toISOString().slice(0, DATE_LENGTH);
}

interface ExportPanelProps {
  scope: AuditScope;
  standing: Standing;
}

export function ExportPanel({ scope, standing }: ExportPanelProps) {
  const base = useId();
  const startId = `${base}-start`;
  const entitled = standing.export;
  const { download, pending, refusal, error, reset } = useAuditExport(scope);
  const [start, setStart] = useState(() =>
    isoDate(Date.now() - DEFAULT_RANGE_DAYS * DAY_MILLISECONDS)
  );
  const [end, setEnd] = useState(() => isoDate(Date.now()));
  const [format, setFormat] = useState<ExportFormatValue>(ExportFormat.Ndjson);
  const [attempted, setAttempted] = useState(false);
  const [downloaded, setDownloaded] = useState<string | null>(null);

  const invalid = start === '' || end === '' || start > end;
  const rangeError =
    attempted && invalid ? refusalMessage(AuditCode.ExportRangeInvalid) : undefined;
  const failure = refusal ? refusalMessage(refusal.code) : error;
  const locked = !entitled || pending;

  const touch = () => {
    setDownloaded(null);
    if (error) {
      reset();
    }
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setAttempted(true);
    if (pending || invalid || !entitled) {
      return;
    }
    download({ format, start_date: start, end_date: end }, (file) => setDownloaded(file.filename));
  };

  return (
    <div className="settings-card audit-card audit-export">
      <div className="audit-card-copy">
        <h3 className="audit-card-title">{DOWNLOAD_TITLE}</h3>
        <p className="audit-hint">{DOWNLOAD_DESCRIPTION}</p>
      </div>
      <form className="audit-export-form" onSubmit={submit} noValidate>
        <fieldset className="audit-export-fields" disabled={locked}>
          <Input
            id={startId}
            label={START_DATE_LABEL}
            type="date"
            value={start}
            max={end || undefined}
            disabled={locked}
            error={rangeError}
            onChange={(event) => {
              setStart(event.target.value);
              touch();
            }}
          />
          <Input
            id={`${base}-end`}
            label={END_DATE_LABEL}
            type="date"
            value={end}
            min={start || undefined}
            disabled={locked}
            aria-invalid={rangeError !== undefined}
            aria-describedby={rangeError ? `${startId}-error` : undefined}
            onChange={(event) => {
              setEnd(event.target.value);
              touch();
            }}
          />
          <Select
            id={`${base}-format`}
            label={FORMAT_LABEL}
            value={format}
            options={FORMAT_OPTIONS}
            disabled={locked}
            onValueChange={(next) => {
              setFormat(next as ExportFormatValue);
              touch();
            }}
          />
        </fieldset>
        {failure && (
          <div className="alert alert-error" role="alert">
            {failure}
          </div>
        )}
        <div className="audit-actions">
          {downloaded && (
            <span className="audit-saved" role="status">
              {DOWNLOADED_LABEL} {downloaded}
            </span>
          )}
          <Button type="submit" size="sm" loading={pending} disabled={!entitled}>
            {DOWNLOAD_LABEL}
          </Button>
        </div>
      </form>
      <p className="audit-hint">
        {readOnlyMessage(standing, LicensedFeature.Export) ?? DOWNLOAD_MEMORY_NOTE}
      </p>
    </div>
  );
}
