import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { wirePullRequest } from '../../../test/pullRequests';
import { PullRequestSchema } from '../schemas';
import type { PullRequest, Readiness } from '../types';
import { groupPullRequests, pullRequestKey } from '../utils';
import { PullRequestList } from './PullRequestList';

const base = PullRequestSchema.parse(wirePullRequest);

function pullRequest(
  number: number,
  readiness: Readiness,
  changes: Partial<PullRequest> = {}
): PullRequest {
  return { ...base, number, readiness, title: `Change ${number}`, ...changes };
}

const listed = [
  pullRequest(1, 'ready'),
  pullRequest(2, 'not_ready'),
  pullRequest(3, 'ready'),
  pullRequest(4, 'in_progress'),
  pullRequest(5, 'not_ready', { draft: true }),
];

function renderList(selected: string | null = null, onSelect = mock()) {
  render(
    <PullRequestList groups={groupPullRequests(listed)} selected={selected} onSelect={onSelect} />
  );
  return onSelect;
}

describe('PullRequestList', () => {
  it('groups pull requests by readiness with a count on each header', () => {
    renderList();

    const groups = screen.getAllByRole('region');
    expect(groups.map((group) => group.getAttribute('aria-label'))).toEqual([
      'Ready',
      'In progress',
      'Not ready',
    ]);
    const headings = groups.map((group) => within(group).getByRole('heading', { level: 2 }));
    expect(headings.map((heading) => heading.querySelector('.sr-only')?.textContent)).toEqual([
      '2 pull requests',
      '1 pull request',
      '2 pull requests',
    ]);
    expect(
      headings.map((heading) => heading.querySelector('[aria-hidden="true"]')?.textContent)
    ).toEqual(['2', '1', '2']);
  });

  it('shows the repository, number, author, checks and readiness on each card', () => {
    renderList();

    const card = screen.getByRole('button', { name: /Change 1/ });
    expect(card).toHaveTextContent('acme/zone #1');
    expect(card).toHaveTextContent('octocat');
    expect(card).toHaveTextContent('1 failed · 2/3 passed');
    expect(within(card).getByText('Ready')).toBeInTheDocument();
  });

  it('badges a draft as a draft whatever its readiness', () => {
    renderList();

    expect(
      within(screen.getByRole('button', { name: /Change 5/ })).getByText('Draft')
    ).toBeInTheDocument();
  });

  it('marks the selected card', () => {
    renderList(pullRequestKey(listed[2]));

    expect(screen.getByRole('button', { name: /Change 3/ })).toHaveAttribute(
      'aria-current',
      'true'
    );
    expect(screen.getByRole('button', { name: /Change 1/ })).not.toHaveAttribute('aria-current');
    expect(screen.getByRole('button', { name: /Change 1/ })).not.toHaveAttribute('aria-pressed');
  });

  it('selects a card by click or keyboard', () => {
    const onSelect = renderList();

    fireEvent.click(screen.getByRole('button', { name: /Change 4/ }));
    expect(onSelect).toHaveBeenLastCalledWith(listed[3]);

    fireEvent.keyDown(screen.getByRole('button', { name: /Change 2/ }), { key: 'Enter' });
    expect(onSelect).toHaveBeenLastCalledWith(listed[1]);

    fireEvent.keyDown(screen.getByRole('button', { name: /Change 2/ }), { key: 'a' });
    expect(onSelect).toHaveBeenCalledTimes(2);
  });
});
