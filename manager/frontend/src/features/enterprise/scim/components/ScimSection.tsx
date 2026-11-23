import { Button } from '@zone/ui';
import { useId } from 'react';
import { useScim } from '../hooks';
import { DESCRIPTION, LOADING, RETRY_LABEL, TITLE } from '../labels';
import { ScimProvisioning } from './ScimProvisioning';
import './ScimSection.css';

interface ScimSectionProps {
  organizationId: string;
}

export function ScimSection({ organizationId }: ScimSectionProps) {
  const headingId = useId();
  const { settings, loading, error, refresh } = useScim(organizationId);

  if (loading) {
    return <div className="loading-state">{LOADING}</div>;
  }

  return (
    <section className="settings-section scim-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {TITLE}
          </h2>
          <p className="section-description">{DESCRIPTION}</p>
        </div>
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            {RETRY_LABEL}
          </Button>
        </div>
      )}
      {settings && (
        <ScimProvisioning
          key={organizationId}
          organizationId={organizationId}
          settings={settings}
        />
      )}
    </section>
  );
}
