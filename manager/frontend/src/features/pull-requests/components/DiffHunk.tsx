import type { Hunk } from '../types';
import { keyed, lineIdentity } from '../utils';
import { DiffRow } from './DiffRow';

export interface DiffHunkProps {
  hunk: Hunk;
}

export function DiffHunk({ hunk }: DiffHunkProps) {
  return (
    <div className="pull-request-diff-hunk">
      <div className="pull-request-diff-line" data-kind="header">
        <span className="pull-request-diff-content">{hunk.header}</span>
      </div>
      {keyed(hunk.lines, lineIdentity).map(([key, line]) => (
        <DiffRow key={key} line={line} />
      ))}
    </div>
  );
}
