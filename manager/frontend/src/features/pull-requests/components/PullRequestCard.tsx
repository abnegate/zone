import { Badge } from '@zone/ui';
import type { KeyboardEvent } from 'react';
import { absoluteTime, relativeTime } from '../../../shared/utils/time';
import type { PullRequest } from '../types';
import { CHECK_TINTS, cardBadge, summarizeChecks } from '../utils';

const ACTIVATION_KEYS: readonly string[] = ['Enter', ' '];

export interface PullRequestCardProps {
  pullRequest: PullRequest;
  selected: boolean;
  onSelect: (pullRequest: PullRequest) => void;
}

export function PullRequestCard({ pullRequest, selected, onSelect }: PullRequestCardProps) {
  const badge = cardBadge(pullRequest);
  const checks = summarizeChecks(pullRequest.checks);

  const activate = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!ACTIVATION_KEYS.includes(event.key)) return;
    event.preventDefault();
    onSelect(pullRequest);
  };

  return (
    <div
      className={`card--list pull-request-card${selected ? ' selected' : ''}`}
      role="button"
      tabIndex={0}
      aria-current={selected || undefined}
      data-testid="pull-request-card"
      onClick={() => onSelect(pullRequest)}
      onKeyDown={activate}
    >
      <div className="pull-request-card-header">
        <h3 className="pull-request-card-title" title={pullRequest.title}>
          {pullRequest.title}
        </h3>
        <Badge variant={badge.tint}>{badge.label}</Badge>
      </div>
      <p className="pull-request-card-meta">
        <span className="pull-request-card-repository">
          {pullRequest.repository} #{pullRequest.number}
        </span>
        {pullRequest.author && (
          <span className="pull-request-card-author">{pullRequest.author}</span>
        )}
      </p>
      <div className="pull-request-card-footer">
        <span className="pull-request-checks" title={checks}>
          <span
            className="pull-request-dot"
            data-tint={CHECK_TINTS[pullRequest.checks.state]}
            aria-hidden="true"
          />
          <span className="pull-request-checks-summary">{checks}</span>
        </span>
        <time dateTime={pullRequest.updated_at} title={absoluteTime(pullRequest.updated_at)}>
          {relativeTime(pullRequest.updated_at)}
        </time>
      </div>
    </div>
  );
}
