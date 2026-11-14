import { Badge } from '@zone/ui';
import { useId, useState } from 'react';
import { absoluteTime, relativeTime } from '../../../shared/utils/time';
import { CHEVRON_PATH, Icon } from '../../github';
import { useReleasePipeline } from '../hooks/useReleasePipeline';
import type { Release } from '../types';
import { PIPELINE_LABELS } from '../utils/pipeline';
import { ReleasePipelineChips } from './ReleasePipelineChips';

export const PRERELEASE = 'Pre-release';

export const NO_PULLS = 'No pull requests';

export const PIPELINE_SUCCEEDED = 'Pipeline succeeded';

export function pullCount(count: number): string {
  return `${count} ${count === 1 ? 'PR' : 'PRs'}`;
}

export function pipelineSummary(release: Release): string {
  if (release.succeeded) return PIPELINE_SUCCEEDED;
  const [blocker] = release.blockers;
  if (blocker) return blocker.detail;
  return `Pipeline ${PIPELINE_LABELS[release.state].toLowerCase()}`;
}

interface ReleaseCardProps {
  release: Release;
}

export function ReleaseCard({ release }: ReleaseCardProps) {
  const [expanded, setExpanded] = useState(false);
  const pullsId = useId();
  const { watching } = useReleasePipeline(release);
  const { name, tag, url, repository, published_at: publishedAt, pulls, pipeline } = release;
  const title = name?.trim() || tag;
  const count = pulls.length;

  return (
    <article
      className="release-card card"
      data-expanded={expanded || undefined}
      aria-label={`${repository} ${tag}`}
    >
      <div className="release-card-header">
        <div className="release-card-title">
          <a className="release-card-name" href={url} target="_blank" rel="noreferrer">
            {title}
          </a>
          {title !== tag && (
            <Badge variant="outline" className="release-card-tag" title={tag}>
              <span className="release-card-tag-text">{tag}</span>
            </Badge>
          )}
          {release.prerelease && <Badge variant="warning">{PRERELEASE}</Badge>}
          <ReleasePipelineChips pipeline={pipeline} />
        </div>
        <p className="release-card-meta">
          <span className="release-card-repository">{repository}</span>
          <time dateTime={publishedAt} title={absoluteTime(publishedAt)}>
            {relativeTime(publishedAt)}
          </time>
        </p>
        <div className="release-card-footer">
          <span className="release-card-summary" data-watching={watching || undefined}>
            {pipelineSummary(release)}
          </span>
          {count > 0 ? (
            <button
              type="button"
              className="release-card-toggle"
              aria-expanded={expanded}
              aria-controls={pullsId}
              onClick={() => setExpanded((open) => !open)}
            >
              {pullCount(count)}
              <Icon path={CHEVRON_PATH} className="release-card-chevron" />
            </button>
          ) : (
            <span className="release-card-empty">{NO_PULLS}</span>
          )}
        </div>
      </div>
      {count > 0 && (
        <ul id={pullsId} className="release-card-pulls" hidden={!expanded}>
          {pulls.map((pull) => (
            <li key={pull.number} className="release-card-pull">
              <a href={pull.url} target="_blank" rel="noreferrer">
                <span className="release-card-pull-number">#{pull.number}</span>
                <span className="release-card-pull-title">{pull.title}</span>
              </a>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
