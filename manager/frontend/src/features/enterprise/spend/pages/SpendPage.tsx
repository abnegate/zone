import { Button } from '@zone/ui';
import { useId, useState } from 'react';
import { SpendBudgets } from '../components/SpendBudgets';
import { SpendNotice, SpendNoticeVariant } from '../components/SpendNotice';
import { SpendOverview } from '../components/SpendOverview';
import { SpendRange } from '../components/SpendRange';
import { useBudgets, useSpendReport, useSpendTargets } from '../hooks';
import { LOADING_LABEL, PAGE_DESCRIPTION, PAGE_TITLE, RETRY_LABEL } from '../labels';
import { lapsed, unlicensed } from '../license';
import { monthToDate } from '../range';
import type { SpendQuery } from '../schemas';
import '../../../analytics/pages/AnalyticsPage.css';
import './SpendPage.css';

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

export interface SpendPageProps {
  organizationId: string;
}

export function SpendPage({ organizationId }: SpendPageProps) {
  const headingId = useId();
  const [query, setQuery] = useState<SpendQuery>(() => monthToDate());
  const report = useSpendReport(organizationId, query);
  const budgets = useBudgets(organizationId);
  const license = report.response ?? budgets.response;
  const community = license !== undefined && unlicensed(license);
  const targets = useSpendTargets(organizationId, license !== undefined && !community);

  if (report.loading && budgets.loading) {
    return <div className="loading-state">{LOADING_LABEL}</div>;
  }

  return (
    <section className="settings-section spend-page" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {PAGE_TITLE}
          </h2>
          <p className="section-description">{PAGE_DESCRIPTION}</p>
        </div>
      </div>
      {community ? (
        <SpendNotice variant={SpendNoticeVariant.Upgrade} />
      ) : (
        <>
          {license && lapsed(license) && <SpendNotice variant={SpendNoticeVariant.Lapsed} />}
          {report.error && (
            <LoadFailure error={report.error} onRetry={() => void report.refresh()} />
          )}
          {license && (
            <SpendRange
              organizationId={organizationId}
              query={query}
              workspaces={targets.workspaces}
              people={targets.people}
              onChange={setQuery}
            />
          )}
          {report.response?.report && (
            <SpendOverview report={report.response.report} refreshing={report.refreshing} />
          )}
          {budgets.error && (
            <LoadFailure error={budgets.error} onRetry={() => void budgets.refresh()} />
          )}
          {budgets.response && (
            <SpendBudgets
              organizationId={organizationId}
              response={budgets.response}
              targets={targets}
            />
          )}
        </>
      )}
    </section>
  );
}
