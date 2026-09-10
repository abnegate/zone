import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { Citation } from '../types';
import {
  citationAnchorId,
  MessageContent,
  UNRESOLVED_MARKER_NOTE,
  UNSOURCED_LINK_NOTE,
} from './MessageContent';

const CITED = 'https://github.com/owner/repository/pull/12';
const INVENTED = 'https://example.com/does-not-exist';

type IdentifiedCitation = Citation & { identifier?: string | null };

const citation = (overrides: Partial<IdentifiedCitation> = {}): IdentifiedCitation => ({
  kind: 'github_issue',
  title: 'owner/repository#12',
  url: CITED,
  revision: null,
  observed_at: '2026-09-05T00:00:00.000Z',
  complete: true,
  outcome: 'success',
  provenance: 'server_execution',
  ...overrides,
});

describe('MessageContent links', () => {
  it('renders a cited anchor as a working link', () => {
    render(
      <MessageContent
        content={`See [the pull request](${CITED}) for the fix.`}
        links="citations"
        citations={[citation()]}
      />
    );

    const link = screen.getByRole('link', { name: 'the pull request' });
    expect(link).toHaveAttribute('href', CITED);
    expect(screen.queryByTestId('unsourced-link')).toBeNull();
  });

  it('carries rel and target on a cited external link', () => {
    render(
      <MessageContent
        content={`See [docs](${CITED}).`}
        links="citations"
        citations={[citation()]}
      />
    );

    const link = screen.getByRole('link', { name: 'docs' });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  });

  it('adds rel and target to an external link under links="all"', () => {
    const { container } = render(
      <MessageContent content={`See [docs](${INVENTED}).`} links="all" />
    );

    const link = screen.getByRole('link', { name: 'docs' });
    expect(link).toHaveAttribute('href', INVENTED);
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(container.querySelector('.message-md-link-unsourced')).toBeNull();
  });

  it('de-links an uncited anchor but keeps its text', () => {
    const { container } = render(
      <MessageContent
        content={`See [the audit report](${INVENTED}) for proof.`}
        links="citations"
        citations={[citation()]}
      />
    );

    expect(container.querySelector('a')).toBeNull();
    const inert = screen.getByTestId('unsourced-link');
    expect(inert).toHaveTextContent('the audit report');
    expect(inert).toHaveTextContent(UNSOURCED_LINK_NOTE);
    expect(inert).toHaveClass('message-md-link-unsourced');
  });

  it('de-links an uncited bare-URL autolink', () => {
    const { container } = render(
      <MessageContent
        content={`Full details at ${INVENTED} today.`}
        links="citations"
        citations={[citation()]}
      />
    );

    expect(container.querySelector('a')).toBeNull();
    expect(screen.getByTestId('unsourced-link')).toHaveTextContent(INVENTED);
  });

  it('de-links an uncited relative and protocol-relative anchor', () => {
    render(
      <MessageContent
        content={'Read [the page](/wiki/page) and [the mirror](//evil.com/x).'}
        links="citations"
        citations={[citation()]}
      />
    );

    const inert = screen.getAllByTestId('unsourced-link');
    expect(inert).toHaveLength(2);
    expect(inert[0]).toHaveTextContent('the page');
    expect(inert[1]).toHaveTextContent('the mirror');
  });

  it('leaves a URL in a fenced code block unlinked and byte-identical', () => {
    const command = `curl ${INVENTED}?token=1#fragment`;
    const { container } = render(
      <MessageContent
        content={`Run this:\n\n\`\`\`sh\n${command}\n\`\`\`\n`}
        links="citations"
        citations={[citation()]}
      />
    );

    expect(container.querySelector('a')).toBeNull();
    expect(screen.queryByTestId('unsourced-link')).toBeNull();
    expect(container.querySelector('pre code')?.textContent).toBe(`${command}\n`);
  });

  it('leaves a URL in inline code unlinked and byte-identical', () => {
    const command = `curl ${INVENTED}?token=1#fragment`;
    const { container } = render(
      <MessageContent
        content={`Run \`${command}\` now.`}
        links="citations"
        citations={[citation()]}
      />
    );

    expect(container.querySelector('a')).toBeNull();
    expect(screen.queryByTestId('unsourced-link')).toBeNull();
    expect(container.querySelector('code')?.textContent).toBe(command);
  });

  it('renders every anchor inert and unmarked under links="none"', () => {
    const { container } = render(
      <MessageContent
        content={`Check [the pull request](${CITED}) and ${INVENTED} next.`}
        links="none"
        citations={[citation()]}
      />
    );

    expect(container.querySelector('a')).toBeNull();
    expect(screen.queryByTestId('unsourced-link')).toBeNull();
    expect(screen.getByText(/the pull request/)).toBeInTheDocument();
    expect(container.textContent).toContain(INVENTED);
  });

  it('leaves links alone while a reply carries no citations', () => {
    const content = `Full details at ${INVENTED} today.`;

    const absent = render(<MessageContent content={content} links="citations" />);
    expect(absent.container.querySelector('a')).toHaveAttribute('href', INVENTED);

    const empty = render(<MessageContent content={content} links="citations" citations={[]} />);
    expect(empty.container.querySelector('a')).toHaveAttribute('href', INVENTED);

    expect(screen.queryByTestId('unsourced-link')).toBeNull();
  });
});

const WEB_MARKER = '[web:a3f21c]';
const KB_MARKER = '[kb:9F0011AA22]';

const sourced = (identifier: string, title: string): IdentifiedCitation =>
  citation({ identifier, title, kind: 'workspace_document', url: `${INVENTED}/${identifier}` });

describe('MessageContent source markers', () => {
  it('renders a resolved marker as a reference to its chip', () => {
    render(
      <MessageContent
        content={`Deploys ran green ${WEB_MARKER} last night.`}
        links="citations"
        citations={[sourced('web:a3f21c', 'Release log')]}
      />
    );

    const reference = screen.getByTestId('citation-reference');
    expect(reference).toHaveAttribute('href', `#${citationAnchorId('web:a3f21c')}`);
    expect(reference).toHaveAttribute('title', 'Release log');
    expect(reference).toHaveTextContent(WEB_MARKER);
    expect(screen.getByRole('link', { name: `${WEB_MARKER} (source: Release log)` })).toBe(
      reference
    );
    expect(screen.queryByTestId('unresolved-marker')).toBeNull();
  });

  it('matches a marker by identifier rather than by position in the list', () => {
    render(
      <MessageContent
        content={`Second source says so ${KB_MARKER}.`}
        links="citations"
        citations={[
          sourced('web:a3f21c', 'First source'),
          sourced('kb:9f0011aa22', 'Second source'),
        ]}
      />
    );

    const reference = screen.getByTestId('citation-reference');
    expect(reference).toHaveAttribute('href', `#${citationAnchorId('kb:9f0011aa22')}`);
    expect(reference).toHaveAttribute('title', 'Second source');
  });

  it('renders an unresolved marker inert and keeps its text', () => {
    const { container } = render(
      <MessageContent
        content={`The audit passed ${WEB_MARKER} in full.`}
        links="citations"
        citations={[sourced('doc:beef01', 'Something else')]}
      />
    );

    expect(container.querySelector('.message-md-citation-ref')).toBeNull();
    const inert = screen.getByTestId('unresolved-marker');
    expect(inert.tagName).toBe('SPAN');
    expect(inert).toHaveTextContent(WEB_MARKER);
    expect(inert).toHaveTextContent(UNRESOLVED_MARKER_NOTE);
    expect(inert).toHaveAttribute('title', UNRESOLVED_MARKER_NOTE);
    expect(inert).toHaveClass('message-md-citation-unresolved');
  });

  it('renders a marker unresolved while a streaming reply has no citations yet', () => {
    render(<MessageContent content={`Still settling ${WEB_MARKER}`} links="citations" />);

    expect(screen.getByTestId('unresolved-marker')).toHaveTextContent(WEB_MARKER);
  });

  it('leaves a marker in a fenced code block byte-identical', () => {
    const command = `grep ${WEB_MARKER} log.txt`;
    const { container } = render(
      <MessageContent
        content={`Run this:\n\n\`\`\`sh\n${command}\n\`\`\`\n`}
        links="citations"
        citations={[sourced('web:a3f21c', 'Release log')]}
      />
    );

    expect(screen.queryByTestId('citation-reference')).toBeNull();
    expect(screen.queryByTestId('unresolved-marker')).toBeNull();
    expect(container.querySelector('pre code')?.textContent).toBe(`${command}\n`);
  });

  it('leaves a marker in inline code byte-identical', () => {
    const command = `grep ${WEB_MARKER} log.txt`;
    const { container } = render(
      <MessageContent
        content={`Run \`${command}\` now.`}
        links="citations"
        citations={[sourced('web:a3f21c', 'Release log')]}
      />
    );

    expect(screen.queryByTestId('citation-reference')).toBeNull();
    expect(screen.queryByTestId('unresolved-marker')).toBeNull();
    expect(container.querySelector('code')?.textContent).toBe(command);
  });

  it('leaves markers as plain text in reasoning', () => {
    const { container } = render(
      <MessageContent
        content={`Weighing ${WEB_MARKER} against ${KB_MARKER}.`}
        links="none"
        citations={[sourced('web:a3f21c', 'Release log')]}
      />
    );

    expect(screen.queryByTestId('citation-reference')).toBeNull();
    expect(screen.queryByTestId('unresolved-marker')).toBeNull();
    expect(container.textContent).toBe(`Weighing ${WEB_MARKER} against ${KB_MARKER}.`);
  });

  it('transforms a marker inside emphasis but not a malformed one', () => {
    const { container } = render(
      <MessageContent
        content={`**Shipped ${WEB_MARKER}** unlike [web:zz] or [other:a3f21c].`}
        links="citations"
        citations={[sourced('web:a3f21c', 'Release log')]}
      />
    );

    expect(screen.getByTestId('citation-reference').closest('strong')).not.toBeNull();
    expect(screen.queryByTestId('unresolved-marker')).toBeNull();
    expect(container.textContent).toContain('[web:zz]');
    expect(container.textContent).toContain('[other:a3f21c]');
  });
});
