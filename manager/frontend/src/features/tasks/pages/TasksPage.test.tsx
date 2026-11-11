import { afterAll, afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BrowserRouter } from 'react-router-dom';
import { layOutBoard, pressKey } from '../../../test/board';
import { clientModule } from '../../../test/client';
import { chooseMenuItem, openMenu } from '../../../test/menu';
import { chooseSelect } from '../../../test/select';
import type { Source } from '../../../types';
import type { Project } from '../../projects/types';
import { STAGE_IDS, TASK_STATUSES } from '../status';
import type { CreateTaskRequest, Task, TaskRun, TaskRunLog, TaskStatus } from '../types';
import TasksPage from './TasksPage';

const mockGetTasks = mock(() => Promise.resolve([] as Task[]));
const mockGetTask = mock(() => Promise.resolve({} as Task));
const mockCreateTask = mock(() => Promise.resolve({} as Task));
const mockUpdateTask = mock(() => Promise.resolve({} as Task));
const mockDeleteTask = mock(() => Promise.resolve());
const running: TaskRun = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'running',
  current_phase: 'thinking',
  progress_percent: 0,
  error_message: null,
  started_at: '2026-09-05T00:00:00Z',
  completed_at: null,
};
const mockRunTask = mock(() => Promise.resolve(running));
const mockGetTaskRuns = mock(() => Promise.resolve([] as TaskRun[]));
const mockGetTaskRun = mock(() => Promise.resolve(running));
const mockGetTaskRunLogs = mock(() => Promise.resolve([] as TaskRunLog[]));
const mockGetProjects = mock(() => Promise.resolve([] as Project[]));
const mockGetSources = mock(() => Promise.resolve([] as Source[]));
const mockToastError = mock();

const mockWorkspace = {
  id: 'workspace-1',
  name: 'Test Workspace',
  organization_id: 'org-1',
  slug: 'test-workspace',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

let currentWorkspace = mockWorkspace;

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: mock(() => ({
    currentWorkspace,
    loading: false,
  })),
}));

mock.module('../../../api/tasks', () => ({
  tasksApi: {
    getTasks: mockGetTasks,
    getTask: mockGetTask,
    createTask: mockCreateTask,
    updateTask: mockUpdateTask,
    deleteTask: mockDeleteTask,
    runTask: mockRunTask,
    getTaskRuns: mockGetTaskRuns,
    getTaskRun: mockGetTaskRun,
    getTaskRunLogs: mockGetTaskRunLogs,
  },
}));

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProjects: mockGetProjects,
  },
}));

mock.module('../../../api/client', () =>
  clientModule({
    getSources: mockGetSources,
  })
);

mock.module('sonner', () => ({
  toast: {
    success: mock(),
    error: mockToastError,
  },
}));

afterAll(() => {
  mock.restore();
});

afterEach(() => {
  window.history.pushState({}, '', '/');
  currentWorkspace = mockWorkspace;
});

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });

const renderTasksPage = (path = '/tasks') => {
  window.history.pushState({}, '', path);
  const queryClient = createQueryClient();
  const page = () => (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <TasksPage />
      </BrowserRouter>
    </QueryClientProvider>
  );
  const view = render(page());
  return { ...view, again: () => view.rerender(page()) };
};

const mockProjects: Project[] = [
  {
    id: 'proj-1',
    name: 'Project Alpha',
    description: 'First project',
    status: 'active',
    github_repo_url: null,
    source_id: 'src-1',
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-15T00:00:00Z',
  },
  {
    id: 'proj-2',
    name: 'Project Beta',
    description: 'Second project',
    status: 'active',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-02T00:00:00Z',
    updated_at: '2024-01-16T00:00:00Z',
  },
];

const mockSources: Source[] = [
  {
    id: 'src-1',
    name: 'GitHub Repo',
    source_type: 'github',
    category: 'file',
    config: { owner: 'test', repo: 'repo' },
    url: 'https://github.com/test/repo',
    description: null,
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  },
];

const mockTasks: Task[] = [
  {
    id: 'task-1',
    workspace_id: 'workspace-1',
    project_ids: ['proj-1'],
    title: 'Implement login',
    description: 'Add user authentication',
    acceptance_criteria: null,
    status: 'created',
    priority: 1,
    is_agentic: true,
    require_plan_approval: false,
    github_repo_url: null,
    source_id: 'src-1',
    source_ids: [],
    model_name: 'gpt-4',
    dependencies: [],
    started_at: null,
    completed_at: null,
    queued_at: null,
    worker_id: null,
    pr_url: null,
    branch_name: null,
    pr_status: null,
    pr_created_at: null,
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-10T00:00:00Z',
  },
  {
    id: 'task-2',
    workspace_id: 'workspace-1',
    project_ids: ['proj-2'],
    title: 'Fix button styling',
    description: 'Update CSS for buttons',
    acceptance_criteria: null,
    status: 'complete',
    priority: 3,
    is_agentic: false,
    require_plan_approval: false,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    model_name: null,
    dependencies: [],
    started_at: null,
    completed_at: '2024-01-12T00:00:00Z',
    queued_at: null,
    worker_id: null,
    pr_url: 'https://github.com/test/repo/pull/123',
    branch_name: 'feature/fix-button-styling',
    pr_status: 'merged',
    pr_created_at: '2024-01-12T00:00:00Z',
    created_at: '2024-01-11T00:00:00Z',
    updated_at: '2024-01-12T00:00:00Z',
  },
];

const TITLES: Record<TaskStatus, string> = {
  created: 'Draft the spec',
  queued: 'Wait for a worker',
  in_progress: 'Write the code',
  blocked: 'Wait on review access',
  paused: 'Hold the migration',
  review: 'Check the diff',
  complete: 'Ship the release',
  cancelled: 'Drop the legacy flag',
};

const everyStatus: Task[] = TASK_STATUSES.map((status) => ({
  ...mockTasks[1],
  id: `task-${status}`,
  title: TITLES[status],
  status,
  pr_url: null,
  pr_status: null,
  branch_name: null,
}));

function column(name: string): HTMLElement {
  return screen.getByRole('region', { name });
}

function count(name: string): string | null | undefined {
  return column(name).querySelector('.board-column-count')?.textContent;
}

function focusedTitle(): string | undefined {
  const active = document.activeElement;
  if (!active?.classList.contains('task-card-open')) return undefined;
  return active.closest<HTMLElement>('li.task-card')?.dataset.taskId;
}

function openTitle(title: string): Promise<HTMLElement> {
  return screen.findByRole('button', { name: title });
}

function runTask(title: string): Promise<HTMLElement> {
  return chooseMenuItem(`Actions for ${title}`, 'Run');
}

describe('TasksPage', () => {
  beforeEach(() => {
    mockGetTasks.mockReset();
    mockGetTask.mockReset();
    mockCreateTask.mockReset();
    mockUpdateTask.mockReset();
    mockDeleteTask.mockReset();
    mockRunTask.mockReset();
    mockRunTask.mockImplementation(() => Promise.resolve(running));
    mockGetTaskRuns.mockReset();
    mockGetTaskRuns.mockImplementation(() => Promise.resolve([]));
    mockGetTaskRun.mockReset();
    mockGetTaskRun.mockImplementation(() => Promise.resolve(running));
    mockGetTaskRunLogs.mockReset();
    mockGetTaskRunLogs.mockImplementation(() => Promise.resolve([]));
    mockGetProjects.mockReset();
    mockGetSources.mockReset();
    mockToastError.mockReset();
    mockGetTasks.mockImplementation(() => Promise.resolve(mockTasks));
    mockGetTask.mockImplementation((id: string) => {
      const found = mockTasks.find((task) => task.id === id);
      return found ? Promise.resolve(found) : Promise.reject(new Error('Task not found'));
    });
    mockUpdateTask.mockImplementation((id: string, request: Partial<Task>) => {
      const found = mockTasks.find((task) => task.id === id);
      if (!found) return Promise.reject(new Error('Task not found'));
      return Promise.resolve({ ...found, ...request });
    });
    mockGetProjects.mockImplementation(() => Promise.resolve(mockProjects));
    mockGetSources.mockImplementation(() => Promise.resolve(mockSources));
  });

  it('shows a skeleton board while loading', async () => {
    mockGetTasks.mockImplementation(() => new Promise(() => {}));
    renderTasksPage();
    expect(document.querySelector('.board')).toHaveAttribute('aria-hidden', 'true');
    expect(document.querySelectorAll('.board-skeleton-card').length).toBe(14);
  });

  it('shows empty state when no tasks', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('No tasks yet')).toBeInTheDocument();
    });
  });

  it('offers a way back when the project filter matches nothing', async () => {
    mockGetTasks.mockImplementation((_workspace: string, project?: string) =>
      Promise.resolve(project ? [] : mockTasks)
    );
    renderTasksPage();
    await screen.findByText('Implement login');

    chooseSelect('Filter by project', 'Project Beta');

    await waitFor(() => {
      expect(screen.getByText('No tasks match')).toBeInTheDocument();
    });
    expect(screen.queryByText('No tasks yet')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Show all tasks' }));

    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });
    expect(screen.getByRole('combobox', { name: 'Filter by project' })).toHaveTextContent(
      'All Projects'
    );
  });

  it('renders tasks on the board', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
      expect(screen.getByText('Fix button styling')).toBeInTheDocument();
    });
    expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
  });

  it('lays every status out in seven columns with their counts', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve(everyStatus));
    renderTasksPage();
    await screen.findByText(TITLES.created);

    const board = screen.getByRole('region', { name: 'Tasks' });
    expect(
      Array.from(board.querySelectorAll('[data-stage]')).map((stage) =>
        stage.getAttribute('data-stage')
      )
    ).toEqual([...STAGE_IDS]);
    expect(count('Planned')).toBe('2');
    for (const name of [
      'In progress',
      'Blocked',
      'Paused',
      'In review',
      'Completed',
      'Cancelled',
    ]) {
      expect(count(name)).toBe('1');
    }
    expect(within(column('In progress')).getByText(TITLES.in_progress)).toBeInTheDocument();
    expect(within(column('Completed')).getByText(TITLES.complete)).toBeInTheDocument();
    expect(screen.queryByText('No tasks')).not.toBeInTheDocument();
  });

  it('keeps queued tasks in Planned with a Queued chip', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve(everyStatus));
    renderTasksPage();
    await screen.findByText(TITLES.queued);

    const planned = column('Planned');
    expect(within(planned).getByText(TITLES.created)).toBeInTheDocument();
    expect(within(planned).getByText(TITLES.queued)).toBeInTheDocument();
    expect(within(planned).getAllByText('Queued')).toHaveLength(1);
  });

  it('shows paused and cancelled tasks in their own columns', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve(everyStatus));
    renderTasksPage();
    await screen.findByText(TITLES.paused);

    expect(within(column('Paused')).getByText(TITLES.paused)).toBeInTheDocument();
    expect(within(column('Cancelled')).getByText(TITLES.cancelled)).toBeInTheDocument();
  });

  it('marks columns without tasks as empty', async () => {
    renderTasksPage();
    await screen.findByText('Implement login');

    for (const name of ['In progress', 'Blocked', 'Paused', 'In review', 'Cancelled']) {
      expect(within(column(name)).getByText('No tasks')).toBeInTheDocument();
      expect(count(name)).toBe('0');
    }
    expect(within(column('Planned')).queryByText('No tasks')).not.toBeInTheDocument();
    expect(within(column('Completed')).queryByText('No tasks')).not.toBeInTheDocument();
  });

  it('opens the linked task from the page URL', async () => {
    renderTasksPage('/tasks?id=task-1');
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Implement login' })).toBeInTheDocument();
    });
  });

  it('opens an editor section from the card title, not a dialog', async () => {
    renderTasksPage();
    fireEvent.click(await openTitle('Implement login'));

    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Tasks' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'New task' })).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Filter by project' })).not.toBeInTheDocument();
  });

  it('saves only the changed title, inline from the detail heading', async () => {
    renderTasksPage();
    fireEvent.click(await openTitle('Implement login'));

    fireEvent.click(await screen.findByRole('button', { name: 'Rename Implement login' }));
    expect(mockGetTask).toHaveBeenCalledWith('task-1');
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();

    const input = screen.getByRole('textbox', { name: 'Task title' });
    expect(document.activeElement).toBe(input);
    fireEvent.change(input, { target: { value: '  Implement logout  ' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockUpdateTask).toHaveBeenCalledWith('task-1', { title: 'Implement logout' });
    });
    expect(mockUpdateTask).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByRole('button', { name: 'Rename Implement logout' })
    ).toBeInTheDocument();
    expect(document.querySelector('section.task-details')).not.toBeNull();
  });

  it('saves an inline title on blur', async () => {
    renderTasksPage('/tasks?task=task-1');
    fireEvent.click(await screen.findByRole('button', { name: 'Rename Implement login' }));

    const input = screen.getByRole('textbox', { name: 'Task title' });
    fireEvent.change(input, { target: { value: 'Implement logout' } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(mockUpdateTask).toHaveBeenCalledWith('task-1', { title: 'Implement logout' });
    });
    expect(mockUpdateTask).toHaveBeenCalledTimes(1);
  });

  it('cancels an inline title edit with Escape', async () => {
    renderTasksPage('/tasks?task=task-1');
    fireEvent.click(await screen.findByRole('button', { name: 'Rename Implement login' }));

    const input = screen.getByRole('textbox', { name: 'Task title' });
    fireEvent.change(input, { target: { value: 'Implement logout' } });
    fireEvent.keyDown(input, { key: 'Escape' });

    expect(screen.queryByRole('textbox', { name: 'Task title' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rename Implement login' })).toBeInTheDocument();
    expect(document.querySelector('section.task-details')).not.toBeNull();
    expect(mockUpdateTask).not.toHaveBeenCalled();
  });

  it('reverts an inline title left empty', async () => {
    renderTasksPage('/tasks?task=task-1');
    fireEvent.click(await screen.findByRole('button', { name: 'Rename Implement login' }));

    const input = screen.getByRole('textbox', { name: 'Task title' });
    fireEvent.change(input, { target: { value: '   ' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(screen.queryByRole('textbox', { name: 'Task title' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rename Implement login' })).toBeInTheDocument();
    expect(mockUpdateTask).not.toHaveBeenCalled();
  });

  it('keeps an inline title edit open with the error when saving fails', async () => {
    mockUpdateTask.mockImplementation(() => Promise.reject(new Error('Task not found')));
    renderTasksPage('/tasks?task=task-1');
    fireEvent.click(await screen.findByRole('button', { name: 'Rename Implement login' }));

    const input = screen.getByRole('textbox', { name: 'Task title' });
    fireEvent.change(input, { target: { value: 'Implement logout' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByRole('alert')).toHaveTextContent('Task not found');
    expect(screen.getByRole('textbox', { name: 'Task title' })).toHaveValue('Implement logout');
  });

  it('renames a task from the board without opening it', async () => {
    renderTasksPage();
    await screen.findByText('Implement login');

    await chooseMenuItem('Actions for Implement login', 'Rename');
    const input = screen.getByRole('textbox', { name: 'Task title' });
    expect(input).toHaveValue('Implement login');
    fireEvent.change(input, { target: { value: 'Implement logout' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockUpdateTask).toHaveBeenCalledWith('task-1', { title: 'Implement logout' });
    });
    expect(document.querySelector('section.task-details')).toBeNull();
    expect(new URLSearchParams(window.location.search).get('task')).toBeNull();
    await waitFor(() => {
      expect(screen.queryByRole('textbox', { name: 'Task title' })).not.toBeInTheDocument();
    });
    await waitFor(() => expect(focusedTitle()).toBe('task-1'));
  });

  it('cancels a board rename with Escape without opening the task', async () => {
    renderTasksPage();
    await screen.findByText('Implement login');

    await chooseMenuItem('Actions for Implement login', 'Rename');
    const input = screen.getByRole('textbox', { name: 'Task title' });
    fireEvent.click(input);
    fireEvent.change(input, { target: { value: 'Implement logout' } });
    fireEvent.keyDown(input, { key: 'Escape' });

    expect(screen.getByText('Implement login')).toBeInTheDocument();
    expect(document.querySelector('section.task-details')).toBeNull();
    expect(mockUpdateTask).not.toHaveBeenCalled();
    await waitFor(() => expect(focusedTitle()).toBe('task-1'));
  });

  describe('a card moved to another column by a reload', () => {
    let stored: Task[] = mockTasks;

    beforeEach(() => {
      stored = mockTasks;
      mockGetTasks.mockImplementation(() => Promise.resolve(stored));
    });

    async function reloadWith(status: TaskStatus) {
      stored = stored.map((task) => (task.id === 'task-1' ? { ...task, status } : task));
      const loads = mockGetTasks.mock.calls.length;
      await act(async () => {
        window.dispatchEvent(new Event('visibilitychange'));
        await waitFor(() => expect(mockGetTasks.mock.calls.length).toBe(loads + 1));
        await mockGetTasks.mock.results[loads].value;
      });
      await waitFor(() => {
        expect(column('In progress').querySelector('[data-task-id="task-1"]')).not.toBeNull();
      });
    }

    it('keeps the rename it was in the middle of, with its text, caret and focus', async () => {
      renderTasksPage();
      await screen.findByText('Implement login');
      await chooseMenuItem('Actions for Implement login', 'Rename');
      const input = screen.getByRole<HTMLInputElement>('textbox', { name: 'Task title' });
      fireEvent.change(input, { target: { value: 'Implement sign in' } });
      input.setSelectionRange(10, 14);
      fireEvent.select(input);

      await reloadWith('in_progress');

      const moved = within(column('In progress')).getByRole<HTMLInputElement>('textbox', {
        name: 'Task title',
      });
      expect(moved).not.toBe(input);
      expect(moved).toHaveValue('Implement sign in');
      expect(document.activeElement).toBe(moved);
      expect([moved.selectionStart, moved.selectionEnd]).toEqual([10, 14]);
      expect(mockUpdateTask).not.toHaveBeenCalled();

      fireEvent.keyDown(moved, { key: 'Enter' });
      await waitFor(() => {
        expect(mockUpdateTask).toHaveBeenCalledWith('task-1', { title: 'Implement sign in' });
      });
    });

    it('keeps focus on the title of the card that had it', async () => {
      renderTasksPage();
      (await openTitle('Implement login')).focus();

      await reloadWith('in_progress');

      expect(focusedTitle()).toBe('task-1');
      expect(column('In progress').contains(document.activeElement)).toBe(true);
    });

    it('keeps focus on the grip of the card that had it', async () => {
      renderTasksPage();
      (await screen.findByRole('button', { name: 'Move Implement login' })).focus();

      await reloadWith('in_progress');

      expect(document.activeElement).toBe(
        within(column('In progress')).getByRole('button', { name: 'Move Implement login' })
      );
    });

    it('closes the open menu of the card and puts focus back on its trigger', async () => {
      renderTasksPage();
      await screen.findByText('Implement login');
      await openMenu('Actions for Implement login');
      expect(screen.getAllByRole('menuitem')).toHaveLength(3);

      await reloadWith('in_progress');

      expect(screen.queryByRole('menuitem')).not.toBeInTheDocument();
      expect(document.activeElement).toBe(
        within(column('In progress')).getByRole('button', { name: 'Actions for Implement login' })
      );
    });

    it('leaves focus alone when nothing on the card had it', async () => {
      renderTasksPage();
      (await openTitle('Fix button styling')).focus();

      await reloadWith('in_progress');

      expect(focusedTitle()).toBe('task-2');
    });

    it('does not pull focus back to a card the user clicked away from', async () => {
      renderTasksPage();
      const title = await openTitle('Implement login');
      title.focus();
      fireEvent.pointerDown(document.body);
      title.blur();

      await reloadWith('in_progress');

      expect(document.activeElement).toBe(document.body);
    });

    it('leaves a rename nobody typed in unsaved when the title changed underneath it', async () => {
      renderTasksPage();
      await screen.findByText('Implement login');
      await chooseMenuItem('Actions for Implement login', 'Rename');
      stored = stored.map((task) =>
        task.id === 'task-1' ? { ...task, title: 'Add the login page' } : task
      );

      await reloadWith('in_progress');
      fireEvent.blur(screen.getByRole('textbox', { name: 'Task title' }));

      expect(await openTitle('Add the login page')).toBeInTheDocument();
      expect(mockUpdateTask).not.toHaveBeenCalled();
    });
  });

  it('does not open the editor from the Run or Delete menu items', async () => {
    renderTasksPage();
    await screen.findByText('Implement login');

    await runTask('Implement login');
    expect(screen.getByRole('dialog', { name: 'Implement login' })).toBeInTheDocument();
    expect(document.querySelector('section.task-details')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Implement login' })).not.toBeInTheDocument();
    });

    await chooseMenuItem('Actions for Implement login', 'Delete');
    expect(screen.getByRole('dialog', { name: 'Delete "Implement login"?' })).toBeInTheDocument();
    expect(document.querySelector('section.task-details')).toBeNull();
    expect(new URLSearchParams(window.location.search).get('task')).toBeNull();
  });

  it('does not open the editor from View PR', async () => {
    renderTasksPage();
    const link = await screen.findByRole('link', { name: 'View PR' });
    fireEvent.click(link);
    expect(document.querySelector('section.task-details')).toBeNull();
    fireEvent.keyDown(link, { key: 'Enter' });
    expect(document.querySelector('section.task-details')).toBeNull();
    fireEvent.keyDown(link, { key: ' ' });
    expect(document.querySelector('section.task-details')).toBeNull();
    expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
  });

  it('opens the editor from the task query, not a dialog', async () => {
    renderTasksPage('/tasks?task=task-1');
    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(mockGetTask).toHaveBeenCalledWith('task-1');
  });

  it('strips id from a landing URL that also has task', async () => {
    renderTasksPage('/tasks?task=task-1&id=task-1');
    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(new URLSearchParams(window.location.search).get('id')).toBeNull();
    expect(new URLSearchParams(window.location.search).get('task')).toBe('task-1');

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(new URLSearchParams(window.location.search).get('id')).toBeNull();
    expect(new URLSearchParams(window.location.search).get('task')).toBeNull();
  });

  it('replaces the execute dialog with the editor when another card is opened', async () => {
    renderTasksPage('/tasks?id=task-1');
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Implement login' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Fix button styling'));

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
    expect(new URLSearchParams(window.location.search).get('id')).toBeNull();
    expect(new URLSearchParams(window.location.search).get('task')).toBe('task-2');
  });

  it('closes the editor from Close and Cancel and drops the task param', async () => {
    renderTasksPage();
    fireEvent.click(await openTitle('Implement login'));
    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
    expect(new URLSearchParams(window.location.search).get('task')).toBe('task-1');

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(document.querySelector('section.task-details')).toBeNull();
    expect(new URLSearchParams(window.location.search).get('task')).toBeNull();

    fireEvent.click(await openTitle('Implement login'));
    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Dirty description' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(new URLSearchParams(window.location.search).get('task')).toBeNull();
    expect(mockUpdateTask).not.toHaveBeenCalled();
  });

  it('shows the board and an alert when the task query is unknown', async () => {
    renderTasksPage('/tasks?task=missing');
    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument();
      expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: 'New task' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Filter by project' })).toBeInTheDocument();
    expect(document.querySelector('section.task-details')).toBeNull();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('opens the editor from the card title with Enter and Space', async () => {
    const user = userEvent.setup();
    renderTasksPage();
    (await openTitle('Implement login')).focus();
    await user.keyboard('{Enter}');
    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.getByRole('region', { name: 'Tasks' })).toBeInTheDocument();
    });

    (await openTitle('Implement login')).focus();
    await user.keyboard(' ');
    await waitFor(() => {
      expect(document.querySelector('section.task-details')).not.toBeNull();
    });
  });

  it('renders page header', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(screen.getByText('Autonomous agent workflows')).toBeInTheDocument();
  });

  it('displays agentic badge for agentic tasks', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Agentic')).toBeInTheDocument();
    });
  });

  it('names the projects on each card, with fallbacks for none and unknown', async () => {
    mockGetTasks.mockImplementation(() =>
      Promise.resolve([
        ...mockTasks,
        { ...mockTasks[1], id: 'task-orphan', title: 'Orphan', project_ids: [] },
        { ...mockTasks[1], id: 'task-gone', title: 'Gone', project_ids: ['proj-gone'] },
      ])
    );
    renderTasksPage();
    await screen.findByText('Implement login');

    const board = screen.getByRole('region', { name: 'Tasks' });
    expect(within(board).getByText('Project Alpha')).toBeInTheDocument();
    expect(within(board).getByText('Project Beta')).toBeInTheDocument();
    expect(within(board).getByText('No projects')).toBeInTheDocument();
    expect(within(board).getByText('Unknown')).toBeInTheDocument();
  });

  it('displays task priority', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByLabelText('Priority 1')).toHaveTextContent('P1');
      expect(screen.getByLabelText('Priority 3')).toHaveTextContent('P3');
    });
  });

  it('filters by project only', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by project' })).toBeInTheDocument();
    });
    expect(screen.queryByRole('combobox', { name: 'Filter by status' })).not.toBeInTheDocument();

    chooseSelect('Filter by project', 'Project Alpha');

    await waitFor(() => {
      expect(
        mockGetTasks.mock.calls.some(
          ([workspace, project]) => workspace === 'workspace-1' && project === 'proj-1'
        )
      ).toBe(true);
    });
  });

  it('opens create task wizard', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));
    expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
  });

  it('labels the primary New task in sentence case with the plus icon', async () => {
    renderTasksPage();
    const button = await screen.findByRole('button', { name: 'New task' });
    expect(button.querySelector('svg.plus-icon')).not.toBeNull();
  });

  it('labels the wizard title optional, generated from the description', async () => {
    renderTasksPage();
    fireEvent.click(await screen.findByRole('button', { name: 'New task' }));
    fireEvent.click(await screen.findByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    const title = await screen.findByLabelText(/^Title/);
    expect(title.closest('.form-group')?.querySelector('label')).toHaveTextContent('Titleoptional');
    expect(document.getElementById(title.getAttribute('aria-describedby') ?? '')).toHaveTextContent(
      'Leave it blank to have one generated from the description.'
    );
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
  });

  it('creates a task with only a description', async () => {
    mockCreateTask.mockImplementation(() => Promise.resolve(mockTasks[0]));
    renderTasksPage();
    fireEvent.click(await screen.findByRole('button', { name: 'New task' }));
    fireEvent.click(await screen.findByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    fireEvent.change(await screen.findByLabelText('Description'), {
      target: { value: 'Users on Safari are sent back to the login page' },
    });
    fireEvent.change(screen.getByLabelText(/^Title/), { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create Task' }));

    await waitFor(() => {
      expect(mockCreateTask).toHaveBeenCalledTimes(1);
    });
    const [, request] = mockCreateTask.mock.calls[0] as unknown as [string, CreateTaskRequest];
    expect(request.description).toBe('Users on Safari are sent back to the login page');
    expect('title' in request).toBe(false);
  });

  it('disables new task button when no projects', async () => {
    mockGetProjects.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).toBeDisabled();
    });
  });

  it('creates a new task via wizard', async () => {
    const newTask: Task = {
      ...mockTasks[0],
      id: 'task-3',
      title: 'New Task',
      description: 'Task description',
      priority: 2,
      is_agentic: false,
      source_id: null,
      model_name: null,
      created_at: '2024-01-13T00:00:00Z',
      updated_at: '2024-01-13T00:00:00Z',
    };
    mockCreateTask.mockImplementation(() => Promise.resolve(newTask));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByLabelText(/^Title/)).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText(/^Title/), { target: { value: 'New Task' } });
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Task description' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Create Task' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create Task' }));

    await waitFor(() => {
      expect(mockCreateTask).toHaveBeenCalled();
    });
  });

  it('cancels create task wizard', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));
    expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'New Task' })).not.toBeInTheDocument();
    });
  });

  describe('deleting', () => {
    const dialogName = 'Delete "Implement login"?';
    let stored: Task[] = mockTasks;

    function serve(tasks: Task[]) {
      stored = tasks;
      mockGetTasks.mockImplementation(() => Promise.resolve(stored));
      mockDeleteTask.mockImplementation((id: string) => {
        stored = stored.filter((task) => task.id !== id);
        return Promise.resolve(undefined);
      });
    }

    beforeEach(() => {
      serve(mockTasks);
    });

    async function askToDelete(): Promise<HTMLElement> {
      renderTasksPage();
      await screen.findByText('Implement login');
      await chooseMenuItem('Actions for Implement login', 'Delete');
      return screen.getByRole('dialog', { name: dialogName });
    }

    it('deletes a task once the dialog is confirmed', async () => {
      const dialog = await askToDelete();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete task' }));

      await waitFor(() => {
        expect(mockDeleteTask).toHaveBeenCalledWith('task-1');
      });
      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: dialogName })).not.toBeInTheDocument();
      });
    });

    it('moves focus to the next card in the column once the task is deleted', async () => {
      const neighbour = { ...mockTasks[0], id: 'task-3', title: 'Write the docs' };
      serve([...mockTasks, neighbour]);
      const dialog = await askToDelete();
      const loads = mockGetTasks.mock.calls.length;

      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete task' }));

      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: dialogName })).not.toBeInTheDocument();
      });
      expect(screen.queryByRole('button', { name: 'Implement login' })).not.toBeInTheDocument();
      await waitFor(() => expect(focusedTitle()).toBe('task-3'));
      await waitFor(() => expect(mockGetTasks.mock.calls.length).toBeGreaterThan(loads));
      await act(async () => {
        await mockGetTasks.mock.results[loads].value;
      });
      expect(focusedTitle()).toBe('task-3');
    });

    it('moves focus to the column once its last task is deleted', async () => {
      const dialog = await askToDelete();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete task' }));

      await waitFor(() => expect(document.activeElement).toBe(column('Planned')));
      expect(within(column('Planned')).getByText('No tasks')).toBeInTheDocument();
    });

    it('leaves focus where it rests when a task deleted elsewhere drops out of the list', async () => {
      const neighbour = { ...mockTasks[0], id: 'task-3', title: 'Write the docs' };
      serve([...mockTasks, neighbour]);
      renderTasksPage();
      await screen.findByText('Implement login');
      expect(document.activeElement).toBe(document.body);

      stored = stored.filter((task) => task.id !== 'task-1');
      await act(async () => {
        window.dispatchEvent(new Event('visibilitychange'));
      });
      await waitFor(() => {
        expect(screen.queryByText('Implement login')).not.toBeInTheDocument();
      });

      expect(document.activeElement).toBe(document.body);
    });

    it('removes the task at once, without waiting for the list to reload', async () => {
      let loaded = false;
      mockGetTasks.mockImplementation(() => {
        if (loaded) return new Promise<Task[]>(() => {});
        loaded = true;
        return Promise.resolve(mockTasks);
      });
      mockDeleteTask.mockImplementation(() => Promise.resolve(undefined));
      const dialog = await askToDelete();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete task' }));

      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: dialogName })).not.toBeInTheDocument();
      });
      expect(screen.queryByText('Implement login')).not.toBeInTheDocument();
      expect(screen.getByText('Fix button styling')).toBeInTheDocument();
      await waitFor(() => expect(document.activeElement).toBe(column('Planned')));
    });

    it('keeps the task when the dialog is cancelled', async () => {
      const dialog = await askToDelete();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: dialogName })).not.toBeInTheDocument();
      });
      expect(mockDeleteTask).not.toHaveBeenCalled();
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    it('shows a failed delete inside the dialog and keeps it open', async () => {
      mockDeleteTask.mockImplementation(() => Promise.reject(new Error('Delete failed')));
      const dialog = await askToDelete();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete task' }));

      expect(await within(dialog).findByRole('alert')).toHaveTextContent('Delete failed');
      expect(screen.getByRole('dialog', { name: dialogName })).toBeInTheDocument();
    });
  });

  it('shows only the error when loading fails, not an empty board', async () => {
    mockGetTasks.mockImplementation(() => Promise.reject(new Error('Network error')));
    renderTasksPage();
    expect(await screen.findByRole('alert')).toHaveTextContent('Network error');
    expect(screen.queryByText('No tasks yet')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create Task' })).not.toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Tasks' })).not.toBeInTheDocument();
  });

  it('keeps the board under the error when a reload fails', async () => {
    let failing = false;
    mockGetTasks.mockImplementation(() =>
      failing ? Promise.reject(new Error('Network error')) : Promise.resolve(mockTasks)
    );
    renderTasksPage();
    await screen.findByText('Implement login');

    failing = true;
    await act(async () => {
      window.dispatchEvent(new Event('visibilitychange'));
    });

    expect(await screen.findByRole('alert')).toHaveTextContent('Network error');
    expect(screen.getByText('Implement login')).toBeInTheDocument();
  });

  it('disables Create Task in the empty state when there is no project to put a task in', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve([]));
    mockGetProjects.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();

    expect(await screen.findByRole('button', { name: 'Create Task' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'New task' })).toBeDisabled();
  });

  it('opens the wizard from Create Task in the empty state', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Create Task' }));

    expect(await screen.findByRole('dialog', { name: 'New Task' })).toBeInTheDocument();
  });

  it('clears the project filter when the workspace changes', async () => {
    const { again } = renderTasksPage();
    await screen.findByText('Implement login');
    chooseSelect('Filter by project', 'Project Beta');
    await waitFor(() => expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-2'));

    currentWorkspace = { ...mockWorkspace, id: 'workspace-2', name: 'Other Workspace' };
    again();

    await waitFor(() => expect(mockGetTasks).toHaveBeenCalledWith('workspace-2', undefined));
    expect(screen.getByRole('combobox', { name: 'Filter by project' })).toHaveTextContent(
      'All Projects'
    );
    expect(mockGetTasks).not.toHaveBeenCalledWith('workspace-2', 'proj-2');
  });

  it('returns to the card and the scroll position it left when the editor closes', async () => {
    renderTasksPage();
    const title = await openTitle('Implement login');
    const board = screen.getByRole('region', { name: 'Tasks' });
    const body = column('Planned').querySelector<HTMLElement>('.board-column-body');
    if (!body) throw new Error('The Planned column has no body');
    board.scrollLeft = 320;
    body.scrollTop = 48;

    fireEvent.click(title);
    await waitFor(() => expect(document.querySelector('section.task-details')).not.toBeNull());
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    await waitFor(() => expect(focusedTitle()).toBe('task-1'));
    expect(screen.getByRole('region', { name: 'Tasks' })).not.toBe(board);
    expect(screen.getByRole('region', { name: 'Tasks' }).scrollLeft).toBe(320);
    expect(column('Planned').querySelector('.board-column-body')?.scrollTop).toBe(48);
  });

  it('does not take focus for a card when the editor was opened from a link', async () => {
    renderTasksPage('/tasks?task=task-1');
    await waitFor(() => expect(document.querySelector('section.task-details')).not.toBeNull());

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    await screen.findByRole('region', { name: 'Tasks' });
    expect(focusedTitle()).toBeUndefined();
  });

  it('opens the execution dialog from the Run menu item', async () => {
    renderTasksPage();
    await screen.findByText('Implement login');

    await runTask('Implement login');

    expect(screen.getByRole('dialog', { name: 'Implement login' })).toBeInTheDocument();
  });

  it('closes task execution view', async () => {
    renderTasksPage();
    await screen.findByText('Implement login');

    await runTask('Implement login');
    expect(screen.getByRole('dialog', { name: 'Implement login' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Implement login' })).not.toBeInTheDocument();
    });
  });

  it('shows agentic styling for agentic task cards', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const agenticCard = screen.getByText('Implement login').closest('.task-card');
    expect(agenticCard).toHaveClass('task-card-agentic');
    expect(screen.getByText('Fix button styling').closest('.task-card')).not.toHaveClass(
      'task-card-agentic'
    );
  });

  it('enables agentic mode in create wizard', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByLabelText(/^Title/)).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText(/^Title/), { target: { value: 'Test Task' } });
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Test description' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    const checkbox = await screen.findByRole('checkbox', { name: /Enable Agentic Mode/i });
    fireEvent.click(checkbox);

    expect(screen.getByLabelText('Code Source')).toBeInTheDocument();
  });

  it('shows project source info when agentic mode enabled', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByLabelText(/^Title/)).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText(/^Title/), { target: { value: 'Test Task' } });
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Test description' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    const checkbox = await screen.findByRole('checkbox', { name: /Enable Agentic Mode/i });
    fireEvent.click(checkbox);

    expect(screen.getByText(/Project uses:/)).toBeInTheDocument();
  });

  it('does not show fictional phases or logs after a failed start', async () => {
    mockRunTask.mockRejectedValueOnce(new Error('Worker unavailable'));
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    fireEvent.click(await screen.findByRole('button', { name: 'Start Execution' }));
    expect(await screen.findByText('Worker unavailable')).toBeInTheDocument();
    expect(screen.queryByText('Architect Planning')).not.toBeInTheDocument();
    expect(screen.queryByText('Execution Logs')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try Again' })).toBeInTheDocument();
  });

  it('preserves a start failure when only historical completed runs exist', async () => {
    const latest: TaskRun = { ...running, status: 'completed' };
    const older: TaskRun = { ...latest, id: 'older', started_at: '2025-01-01T00:00:00Z' };
    mockGetTaskRuns.mockResolvedValue([latest, older]);
    mockGetTaskRun.mockResolvedValue(latest);
    mockRunTask.mockRejectedValueOnce(new Error('Worker unavailable'));
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    const button = await screen.findByRole('button', { name: 'Run Again' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(await screen.findByText('Worker unavailable')).toBeInTheDocument();
    await waitFor(() => expect(mockGetTaskRuns).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('button', { name: 'Try Again' })).toBeEnabled();
    expect(mockGetTaskRun).toHaveBeenCalledTimes(1);
  });

  it('restores an active run and its real activity on reopen', async () => {
    mockGetTaskRuns.mockResolvedValue([running]);
    mockGetTaskRunLogs.mockResolvedValue([
      {
        id: 'log-1',
        run_id: 'run-1',
        phase: 'thinking',
        agent_type: 'worker',
        level: 'info',
        message: 'Reading project files',
        created_at: '2026-09-05T00:00:00Z',
      },
    ]);
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    expect(await screen.findByText('Reading project files')).toBeInTheDocument();
    expect(screen.getByText('worker · info')).toBeInTheDocument();
    expect(screen.getByText('Running')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Stop Execution' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Implement login' })).not.toBeInTheDocument();
    });
    await runTask('Implement login');
    expect(await screen.findByText('Reading project files')).toBeInTheDocument();
    expect(mockRunTask).not.toHaveBeenCalled();
  });

  it('polls real activity until completion without starting another run', async () => {
    mockGetTaskRuns.mockResolvedValue([running]);
    mockGetTaskRun.mockResolvedValueOnce(running);
    mockGetTaskRun.mockResolvedValue({
      ...running,
      status: 'completed',
      current_phase: 'complete',
    });
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    expect(await screen.findByText('Running')).toBeInTheDocument();
    await waitFor(
      () => expect(screen.getByText('Task completed successfully.')).toBeInTheDocument(),
      { timeout: 4000 }
    );
    expect(mockGetTaskRun).toHaveBeenCalledTimes(2);
    expect(mockRunTask).not.toHaveBeenCalled();
  });

  it('shows terminal worker failure separately from monitoring failure', async () => {
    mockGetTaskRuns.mockResolvedValue([running]);
    mockGetTaskRun.mockResolvedValue({
      ...running,
      status: 'failed',
      error_message: 'Model rejected request',
    });
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    expect(await screen.findByText('Model rejected request')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Run Again' })).toBeEnabled();
    expect(screen.queryByText('Monitoring unavailable')).not.toBeInTheDocument();
  });

  it('blocks duplicate starts when the initial history cannot be verified', async () => {
    mockGetTaskRuns.mockRejectedValue(new Error('Network offline'));
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    expect(await screen.findByText('Monitoring unavailable')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Start Execution' })).toBeDisabled();
    expect(screen.queryByText('Task failed')).not.toBeInTheDocument();
  });

  it('aborts a pending start on close and ignores its late completion', async () => {
    let resolve!: (run: TaskRun) => void;
    mockRunTask.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    const button = await screen.findByRole('button', { name: 'Start Execution' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    fireEvent.click(button);
    expect(mockRunTask).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect((mockRunTask.mock.calls[0] as unknown as [string, AbortSignal])[1].aborted).toBe(true);
    resolve(running);
    await Promise.resolve();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(mockGetTaskRun).not.toHaveBeenCalled();
  });

  it('shows create error in wizard', async () => {
    mockCreateTask.mockImplementation(() => Promise.reject(new Error('Create failed')));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByLabelText(/^Title/)).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText(/^Title/), { target: { value: 'New Task' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Description' } });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Create Task' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create Task' }));

    await waitFor(() => {
      expect(screen.getByText('Create failed')).toBeInTheDocument();
    });
  });

  it('displays PR status badge when PR exists', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('PR merged')).toBeInTheDocument();
    });
  });

  it('displays PR link when pr_url exists', async () => {
    renderTasksPage();
    await waitFor(() => {
      const prLink = screen.getByRole('link', { name: 'View PR' });
      expect(prLink).toHaveAttribute('href', 'https://github.com/test/repo/pull/123');
      expect(prLink).toHaveAttribute('target', '_blank');
      expect(prLink).toHaveAttribute('rel', 'noopener noreferrer');
      expect(prLink.closest('.task-card')).not.toBeNull();
    });
  });

  it('shows the pull request status before a link exists', async () => {
    mockGetTasks.mockImplementation(() =>
      Promise.resolve([{ ...mockTasks[0], pr_status: 'pending', pr_url: null }])
    );

    renderTasksPage();
    expect(await screen.findByText('PR pending')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'View PR' })).not.toBeInTheDocument();
  });

  it('shows no pull request on a card without one', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve([mockTasks[0]]));

    renderTasksPage();
    await screen.findByText('Implement login');
    expect(screen.queryByRole('link', { name: 'View PR' })).not.toBeInTheDocument();
    expect(screen.queryByText(/^PR /)).not.toBeInTheDocument();
  });

  it('displays every pull request status', async () => {
    mockGetTasks.mockImplementation(() =>
      Promise.resolve([
        { ...mockTasks[0], id: 'task-pending', pr_status: 'pending' },
        { ...mockTasks[0], id: 'task-open', pr_status: 'open' },
        { ...mockTasks[0], id: 'task-closed', pr_status: 'closed' },
      ])
    );

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('PR pending')).toBeInTheDocument();
      expect(screen.getByText('PR open')).toBeInTheDocument();
      expect(screen.getByText('PR closed')).toBeInTheDocument();
    });
  });

  it('shows the receipt a run recorded, not the log line it arrived on', async () => {
    const completed: TaskRun = { ...running, status: 'completed', current_phase: 'complete' };
    mockRunTask.mockResolvedValueOnce(completed);
    mockGetTaskRun.mockResolvedValue(completed);
    mockGetTaskRunLogs.mockResolvedValue([
      {
        id: 'log-1',
        run_id: 'run-1',
        phase: 'acting',
        agent_type: 'tool',
        level: 'info',
        message: 'Workspace action receipt',
        metadata: {
          action_receipt: {
            id: 'call_1',
            action: 'create_document',
            target_type: 'document',
            target_id: 'doc-1',
            target_label: 'Release checklist',
            actor_id: 'user-1',
            actor_name: 'Owner One',
            occurred_at: '2026-09-09T08:50:00.000Z',
            success: true,
            outcome: 'Document created',
            href: '/wiki?id=doc-1',
          },
        },
        created_at: '2026-09-09T08:50:00.000Z',
      },
    ]);

    renderTasksPage();
    await screen.findByText('Implement login');
    await runTask('Implement login');
    fireEvent.click(await screen.findByRole('button', { name: 'Start Execution' }));

    const receipt = await screen.findByTestId('action-receipt');
    expect(receipt).toHaveTextContent('Created document');
    expect(receipt).toHaveTextContent('Release checklist');
    expect(receipt).toHaveTextContent('Document created');
    expect(screen.getByTestId('action-receipt-link')).toHaveAttribute('href', '/wiki?id=doc-1');
    expect(screen.queryByText('Workspace action receipt')).not.toBeInTheDocument();
  });

  describe('dragging', () => {
    const refusal = 'Task has an active run or is no longer available';
    let restore: () => void = () => {};

    beforeEach(() => {
      restore = layOutBoard();
    });

    afterEach(() => {
      restore();
    });

    async function dragRight(title: string) {
      const handle = await screen.findByRole('button', { name: `Move ${title}` });
      handle.focus();
      await pressKey(handle, 'Space');
      await pressKey(handle, 'ArrowRight');
      await pressKey(handle, 'Space');
    }

    it('moves a paused task to In review by keyboard with a status-only update', async () => {
      let stored = everyStatus;
      mockGetTasks.mockImplementation(() => Promise.resolve(stored));
      mockUpdateTask.mockImplementation((id: string, request: Partial<Task>) => {
        stored = stored.map((task) => (task.id === id ? { ...task, ...request } : task));
        return Promise.resolve(stored.find((task) => task.id === id) as Task);
      });
      renderTasksPage();
      await screen.findByText(TITLES.paused);

      await dragRight(TITLES.paused);

      await waitFor(() => {
        expect(mockUpdateTask).toHaveBeenCalledWith('task-paused', { status: 'review' });
      });
      expect(mockUpdateTask).toHaveBeenCalledTimes(1);
      await waitFor(() => {
        expect(within(column('In review')).getByText(TITLES.paused)).toBeInTheDocument();
      });
      expect(within(column('Paused')).getByText('No tasks')).toBeInTheDocument();
      expect(mockToastError).not.toHaveBeenCalled();
    });

    it('keeps a moved card in its column when the list reloads before the move is answered', async () => {
      let answer: (task: Task) => void = () => {};
      mockGetTasks.mockImplementation(() => Promise.resolve(everyStatus));
      mockUpdateTask.mockImplementation(
        () =>
          new Promise<Task>((resolve) => {
            answer = resolve;
          })
      );
      renderTasksPage();
      await screen.findByText(TITLES.paused);

      await dragRight(TITLES.paused);
      expect(within(column('In review')).getByText(TITLES.paused)).toBeInTheDocument();

      const loads = mockGetTasks.mock.calls.length;
      await act(async () => {
        window.dispatchEvent(new Event('visibilitychange'));
        await waitFor(() => expect(mockGetTasks.mock.calls.length).toBe(loads + 1));
        await mockGetTasks.mock.results[loads].value;
      });

      expect(within(column('In review')).getByText(TITLES.paused)).toBeInTheDocument();
      expect(within(column('Paused')).getByText('No tasks')).toBeInTheDocument();

      const moved = { ...everyStatus.find((task) => task.id === 'task-paused'), status: 'review' };
      mockGetTasks.mockImplementation(() =>
        Promise.resolve(everyStatus.map((task) => (task.id === moved.id ? (moved as Task) : task)))
      );
      await act(async () => {
        answer(moved as Task);
      });
      await waitFor(() => expect(mockGetTasks.mock.calls.length).toBeGreaterThan(loads + 1));
      expect(within(column('In review')).getByText(TITLES.paused)).toBeInTheDocument();
      expect(mockToastError).not.toHaveBeenCalled();
    });

    it('puts a refused move back in its column and says why, before any reload', async () => {
      let loaded = false;
      mockGetTasks.mockImplementation(() => {
        if (loaded) return new Promise<Task[]>(() => {});
        loaded = true;
        return Promise.resolve(everyStatus);
      });
      let refuse: (reason: Error) => void = () => {};
      mockUpdateTask.mockImplementation(
        () =>
          new Promise<Task>((_resolve, reject) => {
            refuse = reject;
          })
      );
      renderTasksPage();
      await screen.findByText(TITLES.paused);

      await dragRight(TITLES.paused);
      expect(within(column('In review')).getByText(TITLES.paused)).toBeInTheDocument();
      expect(within(column('Paused')).queryByText(TITLES.paused)).not.toBeInTheDocument();

      await act(async () => {
        refuse(new Error(refusal));
      });

      await waitFor(() => {
        expect(mockToastError).toHaveBeenCalledWith(`Couldn't move ${TITLES.paused}`, {
          description: refusal,
        });
      });
      expect(mockUpdateTask.mock.calls).toEqual([['task-paused', { status: 'review' }]]);
      expect(
        await screen.findByText(`${TITLES.paused} could not be moved; back in Paused.`)
      ).toHaveAttribute('aria-live', 'assertive');
      expect(within(column('Paused')).getByText(TITLES.paused)).toBeInTheDocument();
      expect(within(column('In review')).queryByText(TITLES.paused)).not.toBeInTheDocument();
    });
  });

  describe('a title generated from the description', () => {
    const placeholder = 'Users on Safari are sent back to the login page';
    const generated = 'Fix Safari login redirect loop';
    const untitled = (): Task => ({
      ...mockTasks[0],
      id: 'task-untitled',
      title: placeholder,
      title_pending: true,
      description: placeholder,
      created_at: new Date().toISOString(),
    });

    it('replaces the placeholder on the board when it arrives', async () => {
      const pending = untitled();
      let arrived = false;
      mockGetTasks.mockImplementation(() =>
        Promise.resolve([
          arrived ? { ...pending, title: generated, title_pending: false } : pending,
        ])
      );
      renderTasksPage();

      const shown = await screen.findByText(placeholder, { selector: '.task-card-open' });
      expect(shown).toHaveClass('task-card-title-pending');
      expect(shown).toHaveAttribute('aria-busy', 'true');
      arrived = true;

      expect(await screen.findByText(generated, {}, { timeout: 5000 })).not.toHaveClass(
        'task-card-title-pending'
      );
      const calls = mockGetTasks.mock.calls.length;
      await new Promise((resolve) => setTimeout(resolve, 2500));
      expect(mockGetTasks.mock.calls.length).toBe(calls);
    }, 15_000);

    it('stops waiting on a placeholder older than two minutes', async () => {
      mockGetTasks.mockImplementation(() =>
        Promise.resolve([
          { ...untitled(), created_at: new Date(Date.now() - 180_000).toISOString() },
        ])
      );
      renderTasksPage();

      await screen.findByText(placeholder, { selector: '.task-card-open' });
      await new Promise((resolve) => setTimeout(resolve, 2500));
      expect(mockGetTasks).toHaveBeenCalledTimes(1);
    }, 10_000);

    it('leaves a generated title that arrives while the rename box is open untouched', async () => {
      const pending = untitled();
      let arrived = false;
      mockGetTasks.mockImplementation(() => Promise.resolve([pending]));
      mockGetTask.mockImplementation(() =>
        Promise.resolve(arrived ? { ...pending, title: generated, title_pending: false } : pending)
      );
      renderTasksPage('/tasks?task=task-untitled');

      fireEvent.click(await screen.findByRole('button', { name: `Rename ${placeholder}` }));
      const input = screen.getByRole('textbox', { name: 'Task title' });
      arrived = true;
      await waitFor(() => expect(mockGetTask.mock.calls.length).toBeGreaterThan(1), {
        timeout: 5000,
      });
      await new Promise((resolve) => setTimeout(resolve, 50));
      fireEvent.blur(input);

      const heading = await screen.findByRole('button', { name: `Rename ${generated}` });
      expect(mockUpdateTask).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(heading);
    }, 15_000);

    it('replaces the placeholder in the open task and keeps unsaved edits', async () => {
      const pending = untitled();
      let arrived = false;
      mockGetTasks.mockImplementation(() => Promise.resolve([pending]));
      mockGetTask.mockImplementation(() =>
        Promise.resolve(arrived ? { ...pending, title: generated, title_pending: false } : pending)
      );
      renderTasksPage('/tasks?task=task-untitled');

      await screen.findByRole('button', { name: `Rename ${placeholder}` });
      fireEvent.change(screen.getByLabelText('Description'), {
        target: { value: 'Also on iOS' },
      });
      arrived = true;

      expect(
        await screen.findByRole('button', { name: `Rename ${generated}` }, { timeout: 5000 })
      ).toBeInTheDocument();
      expect(screen.getByLabelText('Description')).toHaveValue('Also on iOS');
      expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
    }, 15_000);
  });
});
