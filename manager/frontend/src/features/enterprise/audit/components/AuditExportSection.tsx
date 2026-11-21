import { Button } from '@zone/ui';
import { useId } from 'react';
import { UpgradeNotice } from '../../components/UpgradeNotice';
import { useAuditDestinations, useAuditRetention } from '../hooks';
import {
  LOADING_LABEL,
  ORGANIZATION_DESCRIPTION,
  RETRY_LABEL,
  SECTION_TITLE,
  SERVER_DESCRIPTION,
} from '../labels';
import { NoticeTone, standingNotice } from '../messages';
import { AuditScopeKind } from '../schemas';
import type { AuditScope, Standing } from '../types';
import { AuditLicenseWarning } from './AuditLicenseWarning';
import { DestinationList } from './DestinationList';
import { ExportPanel } from './ExportPanel';
import { RetentionPanel } from './RetentionPanel';
import './AuditExportSection.css';

interface StandingNoticeProps {
  standing: Standing;
  days: number | null;
}

function StandingNotice({ standing, days }: StandingNoticeProps) {
  const notice = standingNotice(standing, days);
  if (notice === null) {
    return null;
  }
  return notice.tone === NoticeTone.Upgrade ? (
    <UpgradeNotice message={notice.message} action={notice.action} className="audit-notice" />
  ) : (
    <AuditLicenseWarning message={notice.message} action={notice.action} />
  );
}

interface LoadFailureProps {
  error: string;
  onRetry(): void;
}

function LoadFailure({ error, onRetry }: LoadFailureProps) {
  return (
    <div className="alert alert-error alert-inline" role="alert">
      <span>{error}</span>
      <Button type="button" variant="ghost" size="sm" onClick={onRetry}>
        {RETRY_LABEL}
      </Button>
    </div>
  );
}

interface AuditExportSectionProps {
  scope: AuditScope;
}

export function AuditExportSection({ scope }: AuditExportSectionProps) {
  const headingId = useId();
  const destinations = useAuditDestinations(scope);
  const retention = useAuditRetention(scope);
  const standing = destinations.destinations?.standing ?? retention.retention?.standing;

  if (destinations.loading || retention.loading) {
    return <div className="loading-state">{LOADING_LABEL}</div>;
  }

  return (
    <section className="settings-section audit-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {SECTION_TITLE}
          </h2>
          <p className="section-description">
            {scope.kind === AuditScopeKind.Server ? SERVER_DESCRIPTION : ORGANIZATION_DESCRIPTION}
          </p>
        </div>
      </div>
      {standing && <StandingNotice standing={standing} days={retention.retention?.days ?? null} />}
      {destinations.error && (
        <LoadFailure error={destinations.error} onRetry={() => void destinations.refresh()} />
      )}
      {destinations.destinations && (
        <DestinationList scope={scope} response={destinations.destinations} />
      )}
      {retention.error && (
        <LoadFailure error={retention.error} onRetry={() => void retention.refresh()} />
      )}
      {retention.retention && <RetentionPanel scope={scope} retention={retention.retention} />}
      {standing && <ExportPanel scope={scope} standing={standing} />}
    </section>
  );
}
