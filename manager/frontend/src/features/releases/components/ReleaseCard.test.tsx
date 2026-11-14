import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { release } from '../../../test/releases';
import type { Release } from '../types';
import { groupReleasesByDay } from '../utils/grouping';

const watched: Release[] = [];

mock.module('../hooks/useReleasePipeline', () => ({
  useReleasePipeline: (subject: Release) => {
    watched.push(subject);
    return { watching: false, fetching: false };
  },
}));

let card: typeof import('./ReleaseCard');
let group: typeof import('./ReleaseDayGroup');

beforeAll(async () => {
  card = await import('./ReleaseCard');
  group = await import('./ReleaseDayGroup');
});

afterAll(() => {
  mock.restore();
});

const pulls = [
  { number: 101, url: 'https://github.com/acme/zone/pull/101', title: 'Add releases page' },
  { number: 102, url: 'https://github.com/acme/zone/pull/102', title: 'Poll pipelines' },
];

describe('ReleaseCard', () => {
  it('links the release by name and shows its repository and tag', () => {
    const { ReleaseCard } = card;
    render(<ReleaseCard release={release({ name: 'Autumn', tag: 'v2.0.0' })} />);

    const article = screen.getByRole('article', { name: 'acme/zone v2.0.0' });
    expect(within(article).getByRole('link', { name: 'Autumn' })).toHaveAttribute(
      'href',
      'https://github.com/acme/zone/releases/tag/v2.0.0'
    );
    expect(within(article).getByTitle('v2.0.0')).toHaveClass('release-card-tag');
    expect(within(article).getByText('acme/zone')).toBeInTheDocument();
  });

  it('shows the tag once when the release is named after it', () => {
    const { ReleaseCard } = card;
    render(<ReleaseCard release={release({ tag: 'v1.2.3' })} />);
    expect(screen.getAllByText('v1.2.3')).toHaveLength(1);
  });

  it('marks a pre-release', () => {
    const { ReleaseCard, PRERELEASE } = card;
    render(<ReleaseCard release={release({ prerelease: true })} />);
    expect(screen.getByText(PRERELEASE)).toBeInTheDocument();
  });

  it('summarises the pipeline from its verdict', () => {
    const { pipelineSummary, PIPELINE_SUCCEEDED } = card;
    expect(pipelineSummary(release())).toBe(PIPELINE_SUCCEEDED);
    const blocked = release({
      succeeded: false,
      state: 'failed',
      blockers: [{ code: 'runs_failed', detail: 'Release failed' }],
    });
    expect(pipelineSummary(blocked)).toBe('Release failed');
    expect(pipelineSummary(release({ succeeded: false, state: 'running' }))).toBe(
      'Pipeline running'
    );
  });

  it('expands to the included pull requests as rows', () => {
    const { ReleaseCard } = card;
    render(<ReleaseCard release={release({ pulls })} />);

    const toggle = screen.getByRole('button', { name: '2 PRs' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('link', { name: /Add releases page/ })).toBeNull();

    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const rows = document.getElementById(toggle.getAttribute('aria-controls') ?? '');
    expect(rows).not.toBeNull();
    const links = within(rows as HTMLElement).getAllByRole('link');
    expect(links.map((link) => link.textContent)).toEqual([
      '#101Add releases page',
      '#102Poll pipelines',
    ]);
    expect(links[0]).toHaveAttribute('href', pulls[0].url);

    fireEvent.click(toggle);
    expect(screen.queryByRole('link', { name: /Add releases page/ })).toBeNull();
  });

  it('offers no toggle without pull requests', () => {
    const { ReleaseCard, NO_PULLS } = card;
    render(<ReleaseCard release={release()} />);
    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.getByText(NO_PULLS)).toBeInTheDocument();
  });

  it('counts a single pull request in the singular', () => {
    expect(card.pullCount(1)).toBe('1 PR');
    expect(card.pullCount(3)).toBe('3 PRs');
  });

  it('watches the pipeline of every card', () => {
    const { ReleaseCard } = card;
    const subject = release({ id: 77 });
    watched.length = 0;
    render(<ReleaseCard release={subject} />);
    expect(watched.at(-1)).toBe(subject);
  });
});

describe('ReleaseDayGroup', () => {
  it('heads a day with its label and release count', () => {
    const { ReleaseDayGroup, releaseCount } = group;
    const now = new Date('2026-11-13T12:00:00Z');
    const [day] = groupReleasesByDay(
      [release({ id: 1, tag: 'v1.0.0' }), release({ id: 2, tag: 'v1.0.1' })],
      now
    );
    render(<ReleaseDayGroup day={day} />);

    const section = screen.getByRole('region', { name: `Today ${releaseCount(2)}` });
    expect(within(section).getByRole('heading', { level: 2 })).toHaveClass('release-day-header');
    expect(within(section).getAllByRole('article')).toHaveLength(2);
  });
});
