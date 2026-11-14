import type { PullRequestArtifact } from '../hooks';
import type { PullRequest, PullRequestDetail } from '../types';
import { failedChecks } from '../utils';
import { BotCommentRow } from './BotCommentRow';
import { FailedCheckRow } from './FailedCheckRow';
import { PullRequestLoading } from './PullRequestLoading';
import { ThreadRow } from './ThreadRow';

export const NOTHING_BLOCKING = 'No failed checks, open threads or bot reviews to show';

export const MORE_THREADS = 'More threads on GitHub';

export interface PullRequestBlockersProps {
  pullRequest: PullRequest;
  detail: PullRequestArtifact<PullRequestDetail>;
}

export function PullRequestBlockers({ pullRequest, detail }: PullRequestBlockersProps) {
  const failed = failedChecks(pullRequest);
  const threads = detail.data?.threads ?? [];
  const comments = detail.data?.comments ?? [];
  const threadsComplete = detail.data?.pull_request.threads_complete ?? true;
  const showThreads = threads.length > 0 || !threadsComplete;
  const empty = failed.length === 0 && !showThreads && comments.length === 0;

  return (
    <div className="pull-request-blockers">
      {failed.length > 0 && (
        <section className="pull-request-section" aria-label="Failed checks">
          <h3 className="pull-request-section-title">Failed checks</h3>
          <ul className="pull-request-items">
            {failed.map((check) => (
              <FailedCheckRow
                key={`${check.name}:${check.jobId ?? check.url ?? ''}`}
                pullRequest={pullRequest}
                check={check}
              />
            ))}
          </ul>
        </section>
      )}
      {detail.loading ? (
        <PullRequestLoading label="Loading review threads" />
      ) : detail.error ? (
        <div className="error-banner" role="alert">
          {detail.error}
        </div>
      ) : (
        <>
          {showThreads && (
            <section className="pull-request-section" aria-label="Unresolved threads">
              <h3 className="pull-request-section-title">Unresolved threads</h3>
              {threads.length > 0 && (
                <ul className="pull-request-items">
                  {threads.map((thread) => (
                    <ThreadRow key={thread.id} thread={thread} />
                  ))}
                </ul>
              )}
              {!threadsComplete && (
                <a
                  className="pull-request-more"
                  href={pullRequest.url}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {MORE_THREADS}
                </a>
              )}
            </section>
          )}
          {comments.length > 0 && (
            <section className="pull-request-section" aria-label="Bot reviews">
              <h3 className="pull-request-section-title">Bot reviews</h3>
              <ul className="pull-request-items">
                {comments.map((comment) => (
                  <BotCommentRow key={`${comment.reviewer}:${comment.url}`} comment={comment} />
                ))}
              </ul>
            </section>
          )}
          {empty && <p className="pull-request-muted">{NOTHING_BLOCKING}</p>}
        </>
      )}
    </div>
  );
}
