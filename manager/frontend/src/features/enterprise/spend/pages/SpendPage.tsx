import { useId, useState } from 'react';
import { UpgradeNotice } from '../../components/UpgradeNotice';
import { Plan } from '../../schemas';
import { LoadFailure } from '../components/LoadFailure';
import { SpendBudgets } from '../components/SpendBudgets';
import { SpendLapsedNotice } from '../components/SpendLapsedNotice';
import { SpendOverview } from '../components/SpendOverview';
import { SpendRange } from '../components/SpendRange';
import { useBudgets, useSpendReport, useSpendTargets } from '../hooks';
import { LOADING_LABEL, PAGE_DESCRIPTION, PAGE_TITLE, UPGRADE_NOTICE } from '../labels';
import { lapsed, unlicensed } from '../license';
import { monthToDate } from '../range';
import type { SpendQuery } from '../schemas';
import '../../../analytics/pages/AnalyticsPage.css';
import './SpendPage.css';

export interface SpendPageProps {
  organizationId: string;
  editable: boolean;
}

export function SpendPage({ organizationId, editable }: SpendPageProps) {
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
        <UpgradeNotice plan={Plan.Team} message={UPGRADE_NOTICE} className="plan-notice" />
      ) : (
        <>
          {license && lapsed(license) && <SpendLapsedNotice />}
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
              editable={editable && budgets.response.editable}
            />
          )}
        </>
      )}
    </section>
  );
}
