import { Badge, Button, buttonVariants, Tabs, TabsContent, TabsList, TabsTrigger } from '@zone/ui';
import { useState } from 'react';
import PermissionGate from '../../../shared/components/PermissionGate';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { PERMISSIONS } from '../../../shared/types/permissions';
import { absoluteTime, relativeTime } from '../../../shared/utils/time';
import { Icon, shortSha } from '../../github';
import { usePullRequestDetail } from '../hooks';
import { ReadinessSchema } from '../schemas';
import type { PullRequest, ReviewSummary } from '../types';
import { CHECK_TINTS, cardBadge, summarizeChecks } from '../utils';
import { DiffPanel } from './DiffPanel';
import { CLOSE_PATH, EXTERNAL_PATH } from './icons';
import { MergeDialog } from './MergeDialog';
import { PullRequestBlockers } from './PullRequestBlockers';
import { PullRequestCommits } from './PullRequestCommits';

export const DetailTab = {
  Blockers: 'blockers',
  Diff: 'diff',
  Commits: 'commits',
} as const;

export type DetailTab = (typeof DetailTab)[keyof typeof DetailTab];

const TAB_LABELS: Readonly<Record<DetailTab, string>> = {
  blockers: 'Blockers',
  diff: 'Diff',
  commits: 'Commits',
};

const TABS = Object.values(DetailTab);

export const NO_REVIEW = 'No bot review yet';

export const MERGE_UNAVAILABLE = 'Only a ready pull request can be merged';

export const CLOSE_LABEL = 'Close pull request';

function isDetailTab(value: string): value is DetailTab {
  return (TABS as readonly string[]).includes(value);
}

function confidence(review: ReviewSummary): string | null {
  return review.confidence ? `${review.confidence.score}/${review.confidence.scale}` : null;
}

export interface PullRequestDetailProps {
  pullRequest: PullRequest;
  onClose: () => void;
  onMerged?: () => void;
}

export function PullRequestDetail({ pullRequest, onClose, onMerged }: PullRequestDetailProps) {
  const { writable } = useWorkspace();
  const detail = usePullRequestDetail(pullRequest);
  const [tab, setTab] = useState<DetailTab>(DetailTab.Blockers);
  const [merging, setMerging] = useState(false);
  const badge = cardBadge(pullRequest);
  const checks = summarizeChecks(pullRequest.checks);
  const mergeable =
    pullRequest.readiness === ReadinessSchema.enum.ready &&
    !pullRequest.draft &&
    !!pullRequest.head;

  return (
    <aside className="pull-request-details" aria-label={`Pull request #${pullRequest.number}`}>
      <div className="details-header">
        <h2 title={pullRequest.title}>{pullRequest.title}</h2>
        <Button variant="ghost" size="icon" onClick={onClose} aria-label={CLOSE_LABEL}>
          <Icon path={CLOSE_PATH} className="pull-request-icon" />
        </Button>
      </div>

      <div className="details-content">
        <dl className="detail-facts">
          <dt className="detail-label">Status</dt>
          <dd className="detail-value">
            <Badge variant={badge.tint}>{badge.label}</Badge>
          </dd>
          <dt className="detail-label">Repository</dt>
          <dd className="detail-value">
            <a href={pullRequest.url} target="_blank" rel="noopener noreferrer">
              {pullRequest.repository} #{pullRequest.number}
            </a>
          </dd>
          {pullRequest.author && (
            <>
              <dt className="detail-label">Author</dt>
              <dd className="detail-value">{pullRequest.author}</dd>
            </>
          )}
          <dt className="detail-label">Branch</dt>
          <dd className="detail-value">
            <code className="pull-request-branch">{pullRequest.head_ref}</code>
            <span aria-hidden="true">→</span>
            <code className="pull-request-branch">{pullRequest.base_ref}</code>
          </dd>
          <dt className="detail-label">Checks</dt>
          <dd className="detail-value">
            <span
              className="pull-request-dot"
              data-tint={CHECK_TINTS[pullRequest.checks.state]}
              aria-hidden="true"
            />
            {checks}
          </dd>
          <dt className="detail-label">Review</dt>
          <dd className="detail-value">
            {pullRequest.reviews.length === 0
              ? NO_REVIEW
              : pullRequest.reviews.map((review) => (
                  <span key={review.comment_id} className="pull-request-review">
                    {review.reviewer}
                    {confidence(review) && <Badge variant="neutral">{confidence(review)}</Badge>}
                  </span>
                ))}
            {pullRequest.reviews.length > 0 && (
              <Badge variant={pullRequest.review_current ? 'success' : 'warning'}>
                {pullRequest.review_current ? 'Current' : 'Behind head'}
              </Badge>
            )}
          </dd>
          <dt className="detail-label">Threads</dt>
          <dd className="detail-value">{pullRequest.unresolved_threads} unresolved</dd>
          <dt className="detail-label">Updated</dt>
          <dd className="detail-value">
            <time dateTime={pullRequest.updated_at} title={absoluteTime(pullRequest.updated_at)}>
              {relativeTime(pullRequest.updated_at)}
            </time>
          </dd>
          {pullRequest.head && (
            <>
              <dt className="detail-label">Head</dt>
              <dd className="detail-value">
                <code className="pull-request-branch">{shortSha(pullRequest.head)}</code>
              </dd>
            </>
          )}
        </dl>

        {pullRequest.blockers.length > 0 && (
          <ul className="pull-request-blocker-list" aria-label="Blockers">
            {pullRequest.blockers.map((blocker) => (
              <li key={`${blocker.code}:${blocker.detail}`} className="pull-request-blocker">
                <span className="pull-request-dot" data-tint="warning" aria-hidden="true" />
                {blocker.detail}
              </li>
            ))}
          </ul>
        )}

        <Tabs
          value={tab}
          onValueChange={(value) => isDetailTab(value) && setTab(value)}
          className="pull-request-tabs"
        >
          <TabsList>
            {TABS.map((value) => (
              <TabsTrigger key={value} value={value}>
                {TAB_LABELS[value]}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent value={DetailTab.Blockers}>
            <PullRequestBlockers pullRequest={pullRequest} detail={detail} />
          </TabsContent>
          <TabsContent value={DetailTab.Diff}>
            <DiffPanel pullRequest={pullRequest} />
          </TabsContent>
          <TabsContent value={DetailTab.Commits}>
            <PullRequestCommits pullRequest={pullRequest} />
          </TabsContent>
        </Tabs>
      </div>

      <div className="details-actions">
        <a
          className={buttonVariants({ variant: 'secondary' })}
          href={pullRequest.url}
          target="_blank"
          rel="noopener noreferrer"
        >
          <Icon path={EXTERNAL_PATH} className="pull-request-icon" />
          Open on GitHub
        </a>
        {writable && (
          <PermissionGate permission={PERMISSIONS.PULL_REQUESTS.UPDATE}>
            <Button
              onClick={() => setMerging(true)}
              disabled={!mergeable}
              title={mergeable ? undefined : MERGE_UNAVAILABLE}
            >
              Merge
            </Button>
          </PermissionGate>
        )}
      </div>

      {merging && (
        <MergeDialog
          isOpen
          pullRequest={pullRequest}
          onClose={() => setMerging(false)}
          onMerged={onMerged}
        />
      )}
    </aside>
  );
}
