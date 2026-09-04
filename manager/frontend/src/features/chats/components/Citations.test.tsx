import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { Citation } from '../types';
import { Citations } from './Citations';

const citation = (overrides: Partial<Citation> = {}): Citation => ({
  kind: 'github_build',
  title: 'repository main@aaaaaaa',
  url: 'https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  revision: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  observed_at: '2026-09-05T00:00:00.000Z',
  complete: true,
  outcome: 'success',
  ...overrides,
});

describe('Citations', () => {
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
    expect(items[1].querySelector('a')).toHaveAttribute('href', '/wiki');
    expect(screen.getByText('content-hash')).toBeInTheDocument();
  });
});
