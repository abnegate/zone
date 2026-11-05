import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import type { ChatTasks } from '../hooks/useChatTasks';
import type { ChatTask } from '../types';
import { TasksBox } from './TasksBox';

const RUNNING: ChatTask = {
  id: 'task-3',
  title: 'Fix the price rounding in the cart',
  title_pending: false,
  status: 'in_progress',
  is_agentic: true,
  href: '/tasks?id=task-3',
  created_at: '2026-11-05T10:03:00Z',
  updated_at: '2026-11-05T10:03:00Z',
  run: {
    id: 'run-3',
    status: 'running',
    phase: 'acting',
    progress: 45,
    started_at: '2026-11-05T10:03:00Z',
    completed_at: null,
  },
  pull_request: null,
};

const PUBLISHED: ChatTask = {
  ...RUNNING,
  id: 'task-2',
  title: 'Add the widget catalogue',
  status: 'review',
  href: '/tasks?id=task-2',
  run: {
    id: 'run-2',
    status: 'completed',
    phase: 'complete',
    progress: 100,
    started_at: '2026-11-05T10:02:00Z',
    completed_at: '2026-11-05T10:12:00Z',
  },
  pull_request: { url: 'https://github.com/acme/widgets/pull/12', status: 'open' },
};

const NOTED: ChatTask = {
  ...RUNNING,
  id: 'task-1',
  title: 'Write the release notes',
  status: 'created',
  is_agentic: false,
  href: '/tasks?id=task-1',
  run: null,
};

const COLLAPSED_KEY = 'zone:chat-panel:user-1:tasks';

function tasks(listed: ChatTask[], error: string | null = null): ChatTasks {
  return { tasks: listed, error, refresh: mock(async () => undefined) };
}

function Location() {
  const location = useLocation();
  return <p data-testid="location">{`${location.pathname}${location.search}`}</p>;
}

function renderBox(listed: ChatTasks) {
  return render(
    <MemoryRouter initialEntries={['/chats?id=chat-1']}>
      <Routes>
        <Route path="*" element={<TasksBox userId="user-1" tasks={listed} />} />
      </Routes>
      <Location />
    </MemoryRouter>
  );
}

function toggle(count: number): HTMLElement {
  return screen.getByRole('button', { name: `Tasks, ${count}` });
}

describe('TasksBox', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('lists the tasks in the order the server gives them, newest first', () => {
    renderBox(tasks([RUNNING, PUBLISHED, NOTED]));

    const rows = screen.getAllByRole('listitem');
    expect(rows.map((row) => within(row).getAllByRole('link')[0].textContent)).toEqual([
      RUNNING.title,
      PUBLISHED.title,
      NOTED.title,
    ]);
    expect(toggle(3)).toHaveAttribute('aria-expanded', 'true');
  });

  it('shows a running task with its status, what its run is doing and how far it got', () => {
    renderBox(tasks([RUNNING]));

    const row = screen.getByRole('listitem');
    expect(within(row).getByText('In progress')).toBeInTheDocument();
    expect(within(row).getByText('Running, using tools')).toBeInTheDocument();
    expect(within(row).getByText('45%')).toBeInTheDocument();
    expect(
      within(row).getByRole('progressbar', { name: `${RUNNING.title} progress` })
    ).toHaveAttribute('aria-valuenow', '45');
  });

  it('links the pull request a finished run published, and shows no progress for it', () => {
    renderBox(tasks([PUBLISHED]));

    const row = screen.getByRole('listitem');
    expect(within(row).getByText('In review')).toBeInTheDocument();
    expect(within(row).getByText('Run completed')).toBeInTheDocument();
    const published = within(row).getByRole('link', { name: 'widgets #12' });
    expect(published).toHaveAttribute('href', 'https://github.com/acme/widgets/pull/12');
    expect(published).toHaveAttribute('target', '_blank');
    expect(within(row).queryByRole('progressbar')).toBeNull();
  });

  it('shows a task that never ran with only its status', () => {
    renderBox(tasks([NOTED]));

    const row = screen.getByRole('listitem');
    expect(within(row).getByText('Created')).toBeInTheDocument();
    expect(row.querySelector('.chat-task-meta')).toBeNull();
  });

  it('opens the task when its row is chosen', () => {
    renderBox(tasks([RUNNING]));

    fireEvent.click(screen.getByRole('link', { name: RUNNING.title }));

    expect(screen.getByTestId('location')).toHaveTextContent('/tasks?id=task-3');
  });

  it('collapses, and remembers that it was collapsed', () => {
    const { unmount } = renderBox(tasks([RUNNING]));

    fireEvent.click(toggle(1));

    expect(toggle(1)).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('link', { name: RUNNING.title })).toBeNull();
    expect(localStorage.getItem(COLLAPSED_KEY)).toBe('true');
    unmount();
    renderBox(tasks([RUNNING]));
    expect(toggle(1)).toHaveAttribute('aria-expanded', 'false');
  });

  it('says when the tasks could not be read', () => {
    renderBox(tasks([RUNNING], 'Failed to load tasks'));

    expect(screen.getByRole('alert')).toHaveTextContent('Failed to load tasks');
  });
});
