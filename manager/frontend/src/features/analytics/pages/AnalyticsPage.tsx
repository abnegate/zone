import { Button, EmptyState, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import { useSearchParams } from 'react-router-dom';
import PageBar from '../../../shared/components/PageBar/PageBar';
import {
  AnalyticsSkeleton,
  EvaluationSummaryCard,
  FailureBars,
  ModelBreakdownTable,
  PullRequestSummaryCard,
  RunSeriesChart,
  RunTiles,
  SectionCard,
  SourceLeaderboard,
  TaskSummaryCard,
  TruncationNotice,
} from '../components';
import { useAnalytics } from '../hooks';
import { isPeriod, PERIOD_PARAM, PERIODS, periodLabel, periodOf } from '../period';
import type { Overview, TimePeriod } from '../types';
import './AnalyticsPage.css';

export const TITLE = 'Analytics';
export const SUBTITLE = 'How agent work is landing';
export const PERIOD_LABEL = 'Period';
export const NO_WORKSPACE_TITLE = 'No workspace selected';
export const NO_WORKSPACE_DESCRIPTION = 'Choose a workspace to see its analytics.';
export const FAILED_TITLE = 'Analytics could not be loaded';
export const RETRY = 'Try again';
export const NO_RUNS_DESCRIPTION = 'Runs show up here once agents finish work in this workspace.';

export function noRunsTitle(period: TimePeriod): string {
  return `No runs in the last ${periodLabel(period)}`;
}

export default function AnalyticsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const period = periodOf(searchParams.get(PERIOD_PARAM));
  const { overview, loading, refreshing, error, missingWorkspace, retry } = useAnalytics(period);

  const choose = (value: string) => {
    if (!isPeriod(value)) return;
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.set(PERIOD_PARAM, value);
        return next;
      },
      { replace: true }
    );
  };

  const renderRuns = (current: Overview) => {
    if (current.runs.outcomes.total === 0) {
      return (
        <SectionCard title="Runs" wide>
          <EmptyState
            className="analytics-empty"
            title={noRunsTitle(current.period)}
            description={NO_RUNS_DESCRIPTION}
          />
        </SectionCard>
      );
    }
    return (
      <>
        <SectionCard title="Runs" description="Finished runs by outcome" wide>
          <RunSeriesChart series={current.runs.series} period={current.runs.period} />
        </SectionCard>
        <SectionCard title="Failure kinds" description="What failed runs ran into">
          <FailureBars categories={current.runs.outcomes.categories} />
        </SectionCard>
        <SectionCard title="Sources" description="Runs and merged pull requests per source">
          <SourceLeaderboard sources={current.sources} />
        </SectionCard>
        <SectionCard title="Models" description="How each model's runs went" wide>
          <ModelBreakdownTable models={current.models} />
        </SectionCard>
      </>
    );
  };

  const renderBody = () => {
    if (missingWorkspace) {
      return (
        <EmptyState
          className="analytics-empty"
          title={NO_WORKSPACE_TITLE}
          description={NO_WORKSPACE_DESCRIPTION}
        />
      );
    }
    if (!overview) {
      if (loading) return <AnalyticsSkeleton />;
      return (
        <EmptyState
          className="analytics-empty"
          title={FAILED_TITLE}
          description={error ?? undefined}
          action={
            <Button variant="secondary" onClick={retry}>
              {RETRY}
            </Button>
          }
        />
      );
    }
    return (
      <div className="analytics-overview" aria-busy={refreshing || undefined}>
        {error && (
          <div className="error-banner" role="alert">
            {error}
          </div>
        )}
        <TruncationNotice overview={overview} />
        <RunTiles overview={overview} />
        <div className="analytics-grid">
          {renderRuns(overview)}
          <TaskSummaryCard tasks={overview.tasks} />
          <PullRequestSummaryCard pullRequests={overview.pull_requests} />
          <EvaluationSummaryCard evaluations={overview.evaluations} />
        </div>
      </div>
    );
  };

  return (
    <div className="page page--workspace analytics-page">
      <PageBar title={TITLE} subtitle={SUBTITLE}>
        <Tabs value={period} onValueChange={choose} className="analytics-periods">
          <TabsList aria-label={PERIOD_LABEL}>
            {PERIODS.map((option) => (
              <TabsTrigger key={option.value} value={option.value} disabled={missingWorkspace}>
                {option.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </PageBar>
      <div className="page-body analytics-body">{renderBody()}</div>
    </div>
  );
}
