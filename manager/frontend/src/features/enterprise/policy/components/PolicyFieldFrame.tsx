import { type ReactNode, useId } from 'react';
import { EFFECTIVE_SUMMARY, ORGANIZATION_SUMMARY } from '../labels';
import type { PolicySource } from '../source';
import { PolicySourceBadge } from './PolicySourceBadge';

interface PolicyFieldFrameProps {
  label: string;
  description: string;
  source?: PolicySource;
  organization?: string;
  effective?: string;
  control?: ReactNode;
  error?: string;
  errorId?: string;
  children?: ReactNode;
}

export function PolicyFieldFrame({
  label,
  description,
  source,
  organization,
  effective,
  control,
  error,
  errorId: givenErrorId,
  children,
}: PolicyFieldFrameProps) {
  const titleId = useId();
  const ownErrorId = useId();
  const errorId = givenErrorId ?? ownErrorId;

  return (
    <div
      className="policy-field"
      role="group"
      aria-labelledby={titleId}
      aria-describedby={error ? errorId : undefined}
    >
      <div className="policy-field-header">
        <h3 id={titleId} className="policy-field-title">
          {label}
        </h3>
        {source && <PolicySourceBadge source={source} />}
        {control && <div className="policy-field-control">{control}</div>}
      </div>
      <p className="policy-field-description">{description}</p>
      {organization !== undefined && (
        <p className="policy-field-summary">
          <span className="policy-field-summary-label">{ORGANIZATION_SUMMARY}</span>
          <span className="policy-field-summary-value">{organization}</span>
        </p>
      )}
      {effective !== undefined && (
        <p className="policy-field-summary">
          <span className="policy-field-summary-label">{EFFECTIVE_SUMMARY}</span>
          <span className="policy-field-summary-value">{effective}</span>
        </p>
      )}
      {children}
      {error && (
        <p id={errorId} className="policy-field-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
