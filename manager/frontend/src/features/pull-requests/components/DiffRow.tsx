import type { DiffLine } from '../types';
import { LINE_LABELS, MARKERS } from '../utils';

export interface DiffRowProps {
  line: DiffLine;
}

export function DiffRow({ line }: DiffRowProps) {
  const label = LINE_LABELS[line.kind];
  return (
    <div className="pull-request-diff-line" data-kind={line.kind}>
      <span className="pull-request-diff-number" aria-hidden="true">
        {line.old_line ?? ''}
      </span>
      <span className="pull-request-diff-number" aria-hidden="true">
        {line.new_line ?? ''}
      </span>
      <span className="pull-request-diff-marker" aria-hidden="true">
        {MARKERS[line.kind]}
      </span>
      {label && <span className="sr-only">{label}</span>}
      <span className="pull-request-diff-content">{line.content}</span>
    </div>
  );
}
