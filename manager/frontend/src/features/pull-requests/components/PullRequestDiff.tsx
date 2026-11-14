import { Button } from '@zone/ui';
import { useState } from 'react';
import type { Diff } from '../types';
import { plural } from '../utils';
import { DiffFileView } from './DiffFileView';

export const FILE_BATCH = 20;

export const INCOMPLETE_DIFF =
  'Only part of this diff is shown. Open the files on GitHub for the rest.';

export const NO_CHANGES = 'No changes to show';

export interface PullRequestDiffProps {
  diff: Diff;
}

export function PullRequestDiff({ diff }: PullRequestDiffProps) {
  const [shown, setShown] = useState(FILE_BATCH);
  const additions = diff.files.reduce((total, file) => total + file.additions, 0);
  const deletions = diff.files.reduce((total, file) => total + file.deletions, 0);
  const visible = diff.files.slice(0, shown);
  const remaining = diff.files.length - visible.length;

  return (
    <div className="pull-request-diff">
      {!diff.complete && (
        <p className="source-notice" role="note">
          {diff.warning ?? INCOMPLETE_DIFF}
        </p>
      )}
      {diff.files.length === 0 ? (
        <p className="pull-request-muted">{NO_CHANGES}</p>
      ) : (
        <p className="pull-request-diff-summary">
          {plural(diff.files.length, 'file')} changed
          <span className="pull-request-diff-additions">+{additions}</span>
          <span className="pull-request-diff-deletions">-{deletions}</span>
        </p>
      )}
      {visible.map((file) => (
        <DiffFileView key={file.path} file={file} />
      ))}
      {remaining > 0 && (
        <Button
          variant="secondary"
          size="sm"
          className="pull-request-diff-more"
          onClick={() => setShown((count) => count + FILE_BATCH)}
        >
          Show {Math.min(remaining, FILE_BATCH)} more of {plural(remaining, 'file')}
        </Button>
      )}
    </div>
  );
}
