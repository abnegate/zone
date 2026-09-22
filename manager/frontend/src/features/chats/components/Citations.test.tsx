import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { Citation } from '../types';
import { citationAnchorId, formatObservedAt } from '../utils/citations';
import { Citations } from './Citations';

const citation = (overrides: Partial<Citation> = {}): Citation => ({
  kind: 'github_build',
  title: 'repository main@aaaaaaa',
  url: 'https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  revision: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  observed_at: '2026-09-05T00:00:00.000Z',
  complete: true,
  outcome: 'success',
  provenance: 'server_execution',
  ...overrides,
});

describe('Citations', () => {
  it('anchors an identified chip so a marker in the reply can link to it', () => {
    render(
      <Citations
        citations={[
          citation({ identifier: 'web:a3f21c', kind: 'web', url: 'https://example.test/a' }),
          citation(),
        ]}
      />
    );

    const [identified, plain] = screen.getAllByTestId('citation');
    expect(identified).toHaveAttribute('id', citationAnchorId('web:a3f21c'));
    expect(plain).not.toHaveAttribute('id');
  });

  it('prints a document revision as a date beside the observation, not as a raw stamp', () => {
    render(
      <Citations
        citations={[
          citation({
            kind: 'workspace_document',
            title: 'Deployment checklist',
            url: 'knowledge://11111111-1111-1111-1111-111111111111',
            revision: '2026-09-20T20:22:20.046608',
            observed_at: '2026-09-20T20:24:11Z',
            outcome: 'observed',
          }),
        ]}
      />
    );

    const revision = document.querySelector('.citation-revision');
    expect(revision).toHaveClass('citation-revision--time');
    expect(revision).toHaveTextContent(
      `Revised ${formatObservedAt('2026-09-20T20:22:20.046608Z')}`
    );
    expect(screen.queryByText('2026-09-20T20:22:20.046608')).not.toBeInTheDocument();
  });

  it('labels the observation with its evidence word and drops the date it shares with the revision', () => {
    render(
      <Citations
        citations={[
          citation({
            kind: 'workspace_document',
            title: 'Runbook',
            url: 'knowledge://11111111-1111-1111-1111-111111111111',
            revision: '2026-09-20T20:22:20Z',
            observed_at: '2026-09-20T20:24:11Z',
            outcome: 'observed',
          }),
        ]}
      />
    );

    const time = new Date('2026-09-20T20:24:11Z').toLocaleTimeString([], {
      hour: 'numeric',
      minute: '2-digit',
    });
    expect(document.querySelector('.citation-observation')).toHaveTextContent(`Observed ${time}`);
    expect(document.querySelector('.citation-meta')?.textContent).toBe(
      `Revised ${formatObservedAt('2026-09-20T20:22:20Z')}Observed ${time}`
    );
    expect(document.querySelector('time')).toHaveTextContent(time);
  });

  it('leaves the revision out when it would repeat the observation minute', () => {
    render(
      <Citations
        citations={[
          citation({
            kind: 'workspace_document',
            title: 'Deployment checklist',
            url: 'knowledge://11111111-1111-1111-1111-111111111111',
            revision: '2026-09-20T20:24:02',
            observed_at: '2026-09-20T20:24:11Z',
            outcome: 'observed',
          }),
        ]}
      />
    );

    expect(document.querySelector('.citation-revision')).toBeNull();
    expect(document.querySelector('time')).toHaveAttribute('dateTime', '2026-09-20T20:24:11Z');
  });

  it('renders nothing without sources', () => {
    const { container } = render(<Citations citations={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders a clickable source that keeps url, revision and observation time', () => {
    render(<Citations citations={[citation()]} />);

    const link = screen.getByRole('link', { name: /repository main@aaaaaaa/ });
    expect(link).toHaveAttribute(
      'href',
      'https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    );
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(screen.getByText('aaaaaaa')).toBeInTheDocument();
    expect(screen.getByText('Passing')).toBeInTheDocument();
    expect(screen.getByText('GitHub build')).toBeInTheDocument();
    expect(document.querySelector('time')).toHaveAttribute('dateTime', '2026-09-05T00:00:00.000Z');
  });

  it('labels incomplete evidence distinctly from a passing result', () => {
    render(
      <Citations
        citations={[
          citation({
            complete: false,
            outcome: 'incomplete',
            note: 'Observed CI only; required branch checks are not evaluated.',
          }),
          citation({
            kind: 'workspace_document',
            title: 'Guide',
            url: 'knowledge://11111111-1111-1111-1111-111111111111',
            revision: 'content-hash',
            complete: false,
            outcome: 'incomplete',
          }),
        ]}
      />
    );

    const items = screen.getAllByTestId('citation');
    expect(items[0]).toHaveClass('citation--incomplete');
    expect(items[0]).toHaveTextContent('Incomplete evidence');
    expect(items[0]).not.toHaveTextContent('Passing');
    expect(items[1].querySelector('a')).toHaveAttribute(
      'href',
      '/wiki?id=11111111-1111-1111-1111-111111111111'
    );
    expect(screen.getByText('content-hash')).toBeInTheDocument();
  });

  it('renders a protocol-relative source inert rather than as an in-app link', () => {
    render(<Citations citations={[citation({ url: '//evil.example/x' })]} />);

    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.getByTestId('citation').querySelector('a')).toBeNull();
    expect(screen.getByText('repository main@aaaaaaa')).toBeInTheDocument();
  });

  it('marks a model claim and leaves server-proven evidence unmarked', () => {
    const { rerender } = render(<Citations citations={[citation()]} />);
    expect(screen.queryByText(/not verified/i)).not.toBeInTheDocument();

    rerender(<Citations citations={[citation({ provenance: 'model_asserted' })]} />);
    expect(screen.getByText('Claimed by the model, not verified')).toBeInTheDocument();
    expect(screen.getByTestId('citation')).toHaveClass('citation--model-asserted');
  });
});
