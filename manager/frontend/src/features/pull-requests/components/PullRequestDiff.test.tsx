import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { wireDiff } from '../../../test/pullRequests';
import { DiffSchema } from '../schemas';
import type { Diff, DiffFile } from '../types';
import { BINARY_FILE, TRUNCATED_FILE } from './DiffFileView';
import { FILE_BATCH, INCOMPLETE_DIFF, PullRequestDiff } from './PullRequestDiff';

const diff = DiffSchema.parse(wireDiff);
const [file] = diff.files;

function withFiles(files: DiffFile[], changes: Partial<Diff> = {}): Diff {
  return { ...diff, files, ...changes };
}

function numbered(count: number): DiffFile[] {
  return Array.from({ length: count }, (_, index) => ({ ...file, path: `src/file-${index}.rs` }));
}

describe('PullRequestDiff', () => {
  it('marks each line by kind with its old and new line numbers', () => {
    const { container } = render(<PullRequestDiff diff={diff} />);

    const lines = [...container.querySelectorAll('.pull-request-diff-line')];
    expect(lines.map((line) => line.getAttribute('data-kind'))).toEqual([
      'header',
      'context',
      'deletion',
      'addition',
    ]);
    expect(lines[2].textContent).toBe('2-Removed    old();');
    expect(lines[3].textContent).toBe('2+Added    new();');
    expect(screen.getByText('@@ -1,2 +1,2 @@')).toBeInTheDocument();
  });

  it('names added and removed lines for a screen reader and hides the gutter from it', () => {
    const { container } = render(<PullRequestDiff diff={diff} />);
    const [, context, deletion, addition] = [
      ...container.querySelectorAll('.pull-request-diff-line'),
    ];

    expect(deletion.querySelector('.sr-only')?.textContent).toBe('Removed');
    expect(addition.querySelector('.sr-only')?.textContent).toBe('Added');
    expect(context.querySelector('.sr-only')).toBeNull();
    for (const line of [context, deletion, addition]) {
      for (const hidden of line.querySelectorAll(
        '.pull-request-diff-number, .pull-request-diff-marker'
      )) {
        expect(hidden.getAttribute('aria-hidden')).toBe('true');
      }
    }
  });

  it('sums the changes and links each file to GitHub', () => {
    render(<PullRequestDiff diff={diff} />);

    expect(screen.getByText(/1 file changed/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open src/main.rs on GitHub' })).toHaveAttribute(
      'href',
      file.blob_url
    );
    expect(screen.getByText('Modified')).toBeInTheDocument();
  });

  it('shows the server warning on an incomplete diff, or a fallback', () => {
    const { rerender } = render(
      <PullRequestDiff
        diff={withFiles(diff.files, { complete: false, warning: 'Per-file only' })}
      />
    );
    expect(screen.getByRole('note')).toHaveTextContent('Per-file only');

    rerender(<PullRequestDiff diff={withFiles(diff.files, { complete: false, warning: null })} />);
    expect(screen.getByRole('note')).toHaveTextContent(INCOMPLETE_DIFF);
  });

  it('shows no warning on a complete diff', () => {
    render(<PullRequestDiff diff={diff} />);
    expect(screen.queryByRole('note')).not.toBeInTheDocument();
  });

  it('notes binary and truncated files instead of hunks', () => {
    render(
      <PullRequestDiff
        diff={withFiles([
          { ...file, path: 'logo.png', binary: true, hunks: [] },
          { ...file, path: 'big.rs', truncated: true },
        ])}
      />
    );

    expect(screen.getByText(BINARY_FILE)).toBeInTheDocument();
    expect(screen.getByText(TRUNCATED_FILE)).toBeInTheDocument();
  });

  it('collapses and expands a file', () => {
    const { container } = render(<PullRequestDiff diff={diff} />);
    const toggle = screen.getByRole('button', { name: /src\/main\.rs/ });

    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(container.querySelector('.pull-request-diff-line')).toBeNull();
    fireEvent.click(toggle);
    expect(container.querySelector('.pull-request-diff-line')).not.toBeNull();
  });

  it('renders files in batches', () => {
    const { container } = render(
      <PullRequestDiff diff={withFiles(numbered(FILE_BATCH * 2 + 5))} />
    );
    const shown = () => container.querySelectorAll('.pull-request-diff-file').length;

    expect(shown()).toBe(FILE_BATCH);
    fireEvent.click(screen.getByRole('button', { name: 'Show 20 more of 25 files' }));
    expect(shown()).toBe(FILE_BATCH * 2);
    fireEvent.click(screen.getByRole('button', { name: 'Show 5 more of 5 files' }));
    expect(shown()).toBe(FILE_BATCH * 2 + 5);
    expect(screen.queryByRole('button', { name: /more of/ })).not.toBeInTheDocument();
  });

  it('says so when nothing changed', () => {
    render(<PullRequestDiff diff={withFiles([])} />);
    expect(screen.getByText('No changes to show')).toBeInTheDocument();
  });
});
