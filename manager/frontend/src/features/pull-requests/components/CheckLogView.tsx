import { useCheckLog } from '../hooks';
import type { PullRequest } from '../types';
import { PullRequestLoading } from './PullRequestLoading';

export const TRUNCATED_LOG = 'Only the end of this log is shown.';

export interface CheckLogViewProps {
  pullRequest: PullRequest;
  jobId: number;
}

export function CheckLogView({ pullRequest, jobId }: CheckLogViewProps) {
  const log = useCheckLog(pullRequest, jobId);
  if (log.loading) return <PullRequestLoading label="Loading log" />;
  if (log.error) {
    return (
      <div className="error-banner" role="alert">
        {log.error}
      </div>
    );
  }
  if (!log.data) return null;
  return (
    <div className="pull-request-log">
      {log.data.truncated && <p className="pull-request-muted">{TRUNCATED_LOG}</p>}
      <pre className="pull-request-log-text">{log.data.text}</pre>
    </div>
  );
}
