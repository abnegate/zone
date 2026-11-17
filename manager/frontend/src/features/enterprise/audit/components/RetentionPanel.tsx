import { Button, Checkbox, Input } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { absoluteTime, relativeTime } from '../../../../shared/utils/time';
import { useSetAuditRetention } from '../hooks';
import {
  CUTOFF_LABEL,
  EXPIRED_LABEL,
  HELD_LABEL,
  HOLDERS_LABEL,
  KEEP_FOREVER_LABEL,
  LAST_SWEEP_LABEL,
  NOT_SWEPT,
  RETENTION_DAYS_HINT,
  RETENTION_DAYS_LABEL,
  RETENTION_DESCRIPTION,
  RETENTION_NOT_APPLIED,
  RETENTION_SAVED,
  RETENTION_TITLE,
  SAVE_RETENTION_LABEL,
} from '../labels';
import {
  deletedText,
  LicensedFeature,
  readOnlyMessage,
  refusalMessage,
  retentionSummary,
  waitMessage,
} from '../messages';
import { AuditCode, RETENTION_DAYS_MAXIMUM, RETENTION_DAYS_MINIMUM } from '../schemas';
import type { AuditScope, RetentionResponse } from '../types';
import { AuditUpgradeNotice } from './AuditUpgradeNotice';

const INTEGER = /^\d+$/;

function parseDays(text: string): number | null {
  const trimmed = text.trim();
  if (!INTEGER.test(trimmed)) {
    return null;
  }
  const days = Number(trimmed);
  return days >= RETENTION_DAYS_MINIMUM && days <= RETENTION_DAYS_MAXIMUM ? days : null;
}

interface RetentionFormProps {
  scope: AuditScope;
  days: number | null;
}

function RetentionForm({ scope, days }: RetentionFormProps) {
  const base = useId();
  const { set, pending, refusal, error, reset } = useSetAuditRetention(scope);
  const [forever, setForever] = useState(days === null);
  const [text, setText] = useState(days === null ? '' : String(days));
  const [attempted, setAttempted] = useState(false);
  const [saved, setSaved] = useState(false);

  const parsed = parseDays(text);
  const requested = forever ? null : parsed;
  const invalid = !forever && parsed === null;
  const unchanged = !invalid && requested === days;
  const licenseRequired = refusal?.code === AuditCode.LicenseRequired;
  const failure = licenseRequired ? null : refusal ? refusalMessage(refusal.code) : error;
  const fieldError = attempted && invalid ? refusalMessage(AuditCode.RetentionInvalid) : undefined;

  const touch = () => {
    setSaved(false);
    if (error) {
      reset();
    }
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setAttempted(true);
    if (pending || invalid || unchanged) {
      return;
    }
    set(requested, () => setSaved(true));
  };

  return (
    <form className="audit-retention-form" onSubmit={submit} noValidate>
      <div className="audit-retention-fields">
        <Input
          id={`${base}-days`}
          label={RETENTION_DAYS_LABEL}
          inputMode="numeric"
          value={text}
          disabled={pending || forever}
          helpText={RETENTION_DAYS_HINT}
          error={fieldError}
          onChange={(event) => {
            setText(event.target.value);
            touch();
          }}
        />
        <Checkbox
          id={`${base}-forever`}
          label={KEEP_FOREVER_LABEL}
          checked={forever}
          disabled={pending}
          onCheckedChange={(checked) => {
            setForever(checked);
            touch();
          }}
        />
      </div>
      {licenseRequired && (
        <AuditUpgradeNotice message={refusalMessage(AuditCode.LicenseRequired)} />
      )}
      {failure && (
        <div className="alert alert-error" role="alert">
          {failure}
        </div>
      )}
      <div className="audit-actions">
        {saved && (
          <span className="audit-saved" role="status">
            {RETENTION_SAVED}
          </span>
        )}
        <Button type="submit" size="sm" loading={pending} disabled={unchanged}>
          {SAVE_RETENTION_LABEL}
        </Button>
      </div>
    </form>
  );
}

interface RetentionPanelProps {
  scope: AuditScope;
  retention: RetentionResponse;
}

export function RetentionPanel({ scope, retention }: RetentionPanelProps) {
  const { standing, days, cutoff, holders } = retention;
  const applied = standing.applies && days !== null;
  const readOnly = readOnlyMessage(standing, LicensedFeature.Retention);

  return (
    <div className="settings-card audit-card audit-retention">
      <div className="audit-card-copy">
        <h3 className="audit-card-title">{RETENTION_TITLE}</h3>
        <p className="audit-hint">{RETENTION_DESCRIPTION}</p>
      </div>
      <p className="audit-summary">{retentionSummary(days, standing.applies)}</p>
      {!standing.applies && days === null && <p className="audit-hint">{RETENTION_NOT_APPLIED}</p>}
      {applied && (
        <dl className="audit-stats">
          {cutoff && (
            <div className="audit-stat">
              <dt>{CUTOFF_LABEL}</dt>
              <dd>
                <time dateTime={cutoff}>{absoluteTime(cutoff)}</time>
              </dd>
            </div>
          )}
          <div className="audit-stat">
            <dt>{EXPIRED_LABEL}</dt>
            <dd>{retention.expired.toLocaleString()}</dd>
          </div>
          <div className="audit-stat">
            <dt>{HELD_LABEL}</dt>
            <dd>{retention.held.toLocaleString()}</dd>
          </div>
          <div className="audit-stat">
            <dt>{LAST_SWEEP_LABEL}</dt>
            <dd>
              {retention.swept_at ? (
                <>
                  <time dateTime={retention.swept_at} title={absoluteTime(retention.swept_at)}>
                    {relativeTime(retention.swept_at)}
                  </time>
                  , {deletedText(retention.deleted_count)}
                </>
              ) : (
                NOT_SWEPT
              )}
            </dd>
          </div>
        </dl>
      )}
      {applied && holders.length > 0 && (
        <div className="audit-holders">
          <h4 className="audit-holders-title">{HOLDERS_LABEL}</h4>
          <ul className="audit-holder-list">
            {holders.map((holder) => (
              <li key={holder.destination_id} className="audit-holder">
                <span className="audit-holder-name">{holder.name}</span>
                <span className="audit-holder-wait">{waitMessage(holder.wait)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {readOnly === null ? (
        <RetentionForm scope={scope} days={days} />
      ) : (
        <p className="audit-hint">{readOnly}</p>
      )}
    </div>
  );
}
