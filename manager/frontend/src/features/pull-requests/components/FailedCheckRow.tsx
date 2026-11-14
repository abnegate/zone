import { Button } from '@zone/ui';
import { useState } from 'react';
import type { PullRequest } from '../types';
import type { FailedCheck } from '../utils';
import { CheckLogView } from './CheckLogView';
import { GitHubLink } from './GitHubLink';

export interface FailedCheckRowProps {
  pullRequest: PullRequest;
  check: FailedCheck;
}

export function FailedCheckRow({ pullRequest, check }: FailedCheckRowProps) {
  const [open, setOpen] = useState(false);
  const { jobId } = check;
  return (
    <li className="pull-request-item">
      <div className="pull-request-row">
        <span className="pull-request-dot" data-tint="destructive" aria-hidden="true" />
        <span className="pull-request-row-title">{check.name}</span>
        {jobId !== null && (
          <Button
            variant="ghost"
            size="sm"
            aria-expanded={open}
            onClick={() => setOpen((value) => !value)}
          >
            {open ? 'Hide logs' : 'Show logs'}
          </Button>
        )}
        {check.url && <GitHubLink href={check.url} label={`Open ${check.name} on GitHub`} />}
      </div>
      {open && jobId !== null && <CheckLogView pullRequest={pullRequest} jobId={jobId} />}
    </li>
  );
}
