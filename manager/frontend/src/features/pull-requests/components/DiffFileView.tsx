import { Badge } from '@zone/ui';
import { useState } from 'react';
import { CHEVRON_PATH, Icon } from '../../github';
import type { DiffFile } from '../types';
import { hunkIdentity, keyed, STATUS_LABELS, STATUS_TINTS } from '../utils';
import { DiffHunk } from './DiffHunk';
import { EXTERNAL_PATH } from './icons';

export const BINARY_FILE = 'Binary file not shown';

export const TRUNCATED_FILE = 'This file is cut short. Open it on GitHub to read the rest.';

export interface DiffFileViewProps {
  file: DiffFile;
}

export function DiffFileView({ file }: DiffFileViewProps) {
  const [open, setOpen] = useState(true);
  const moved = file.previous_path && file.previous_path !== file.path;

  return (
    <section className="pull-request-diff-file" data-open={open || undefined}>
      <div className="pull-request-diff-file-header">
        <button
          type="button"
          className="pull-request-diff-toggle"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          <Icon path={CHEVRON_PATH} className="pull-request-diff-chevron" />
          <span className="pull-request-diff-path" title={file.path}>
            {moved ? `${file.previous_path} → ${file.path}` : file.path}
          </span>
        </button>
        <Badge variant={STATUS_TINTS[file.status]}>{STATUS_LABELS[file.status]}</Badge>
        <span className="pull-request-diff-stat">
          <span className="pull-request-diff-additions">+{file.additions}</span>
          <span className="pull-request-diff-deletions">-{file.deletions}</span>
        </span>
        <a
          className="pull-request-diff-blob"
          href={file.blob_url}
          target="_blank"
          rel="noopener noreferrer"
          aria-label={`Open ${file.path} on GitHub`}
          title="Open on GitHub"
        >
          <Icon path={EXTERNAL_PATH} />
        </a>
      </div>
      {open && (
        <div className="pull-request-diff-body">
          {file.binary ? (
            <p className="pull-request-diff-note">{BINARY_FILE}</p>
          ) : (
            keyed(file.hunks, hunkIdentity).map(([key, hunk]) => <DiffHunk key={key} hunk={hunk} />)
          )}
          {file.truncated && <p className="pull-request-diff-note">{TRUNCATED_FILE}</p>}
        </div>
      )}
    </section>
  );
}
