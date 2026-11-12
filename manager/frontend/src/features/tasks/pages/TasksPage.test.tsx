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
import { SOURCES_LOCATION } from '../../settings/navigation/sections';
import { STAGE_IDS, TASK_STATUSES } from '../status';
import type { CreateTaskRequest, Task, TaskRun, TaskRunLog, TaskStatus } from '../types';
import TasksPage from './TasksPage';

const mockGetTasks = mock(() => Promise.resolve([] as Task[]));
const mockGetTask = mock(() => Promise.resolve({} as Task));
const mockCreateTask = mock(() => Promise.resolve({} as Task));
const mockUpdateTask = mock(() => Promise.resolve({} as Task));
const mockUpdateTasks = mock(() => Promise.resolve([] as Task[]));
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
const mockToastSuccess = mock();

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
    updateTasks: mockUpdateTasks,
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
    success: mockToastSuccess,
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

const PULL_REQUEST = 'Pull request #123 merged';

const PULL_REQUEST_CHIP = 'PR #123 · merged';

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
    mockUpdateTasks.mockReset();
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
    mockToastSuccess.mockReset();
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

  it('does not open the editor from the pull request link', async () => {
    renderTasksPage();
    const link = await screen.findByRole('link', { name: PULL_REQUEST });
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

  it('marks agentic tasks on their cards', async () => {
    renderTasksPage();
    const mark = await screen.findByRole('img', { name: 'Agentic' });
    expect(mark.closest('.task-card')).toHaveTextContent('Implement login');
  });

  it('names the projects on each card, with a fallback for unknown and nothing for none', async () => {
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
    expect(within(board).getByText('Unknown')).toBeInTheDocument();
    expect(within(board).queryByText('No projects')).not.toBeInTheDocument();
    const orphan = (await openTitle('Orphan')).closest('li.task-card');
    expect(orphan?.querySelector('.task-card-project')).toBeNull();
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

  it('leaves the agentic mark off manual task cards', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const manual = screen.getByText('Fix button styling').closest<HTMLElement>('.task-card');
    expect(manual).not.toBeNull();
    expect(within(manual as HTMLElement).queryByRole('img', { name: 'Agentic' })).toBeNull();
    expect(screen.getAllByRole('img', { name: 'Agentic' })).toHaveLength(1);
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

  it('points to workspace settings when agentic mode has no source to use', async () => {
    mockGetSources.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'New task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: 'New task' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Beta/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Beta/i }));
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

    expect(
      screen.getByText(`No sources configured. Add one in ${SOURCES_LOCATION}.`)
    ).toBeInTheDocument();
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

  it('displays the pull request number and status in one row when a PR exists', async () => {
    renderTasksPage();
    const link = await screen.findByRole('link', { name: PULL_REQUEST });
    expect(link).toHaveClass('task-card-pull-request-merged');
    expect(link).toHaveTextContent('Pull request#123merged');
    expect(screen.getAllByText('Pull request')).toHaveLength(1);
  });

  it('links the pull request row when pr_url exists', async () => {
    renderTasksPage();
    await waitFor(() => {
      const link = screen.getByRole('link', { name: PULL_REQUEST });
      expect(link).toHaveAttribute('href', 'https://github.com/test/repo/pull/123');
      expect(link).toHaveAttribute('target', '_blank');
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');
      expect(link.closest('.task-card')).not.toBeNull();
    });
  });

  it('shows the pull request status before a link exists', async () => {
    mockGetTasks.mockImplementation(() =>
      Promise.resolve([{ ...mockTasks[0], pr_status: 'pending', pr_url: null }])
    );

    renderTasksPage();
    const label = await screen.findByText('Pull request');
    expect(label.closest('.task-card-pull-request')).toHaveTextContent('Pull requestpending');
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('shows no pull request on a card without one', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve([mockTasks[0]]));

    renderTasksPage();
    await screen.findByText('Implement login');
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.queryByText('Pull request')).not.toBeInTheDocument();
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
      const rows = Array.from(document.querySelectorAll('.task-card-pull-request'));
      expect(rows.map((row) => row.textContent)).toEqual([
        'Pull requestpending',
        'Pull requestopen',
        'Pull requestclosed',
      ]);
    });
  });

  describe('related tasks', () => {
    const spec: Task = { ...mockTasks[1], id: 'task-spec', title: 'Write the spec' };
    const login: Task = { ...mockTasks[0], dependencies: ['task-spec', 'task-gone'] };
    const release: Task = {
      ...mockTasks[1],
      id: 'task-release',
      title: 'Ship the release',
      status: 'review',
      dependencies: ['task-1'],
      pr_url: null,
      pr_status: null,
      branch_name: null,
    };
    const linked: Task[] = [login, spec, release];

    beforeEach(() => {
      mockGetTasks.mockImplementation(() => Promise.resolve(linked));
      mockGetTask.mockImplementation((id: string) => {
        const found = linked.find((task) => task.id === id);
        return found ? Promise.resolve(found) : Promise.reject(new Error('Task not found'));
      });
    });

    function cardOf(title: string): HTMLElement {
      const item = screen.getByRole('button', { name: title }).closest<HTMLElement>('li.task-card');
      if (!item) throw new Error(`No card for ${title}`);
      return item;
    }

    async function editing(title: string): Promise<HTMLElement> {
      await screen.findByRole('button', { name: `Rename ${title}` });
      return screen.getByRole('region', { name: 'Links' });
    }

    it('lists on a card what it depends on and what it blocks', async () => {
      renderTasksPage();
      await openTitle('Implement login');

      const host = within(cardOf('Implement login'));
      expect(
        within(host.getByRole('list', { name: 'Depends on' }))
          .getAllByRole('listitem')
          .map((item) => item.textContent)
      ).toEqual(['Write the specCompleted', '1 unknown']);
      expect(
        within(host.getByRole('list', { name: 'Blocks' }))
          .getAllByRole('button')
          .map((button) => button.getAttribute('aria-label'))
      ).toEqual(['Open Ship the release']);
      expect(
        within(cardOf('Write the spec')).getByRole('button', { name: 'Open Implement login' })
      ).toBeInTheDocument();
    });

    it('opens the related task from a card, not the card it is listed on', async () => {
      renderTasksPage();
      await openTitle('Implement login');

      fireEvent.click(
        within(cardOf('Implement login')).getByRole('button', { name: 'Open Write the spec' })
      );

      await screen.findByRole('button', { name: 'Rename Write the spec' });
      expect(new URLSearchParams(window.location.search).get('task')).toBe('task-spec');
      expect(mockGetTask).toHaveBeenCalledWith('task-spec');
      expect(mockGetTask).not.toHaveBeenCalledWith('task-1');
    });

    it('returns to the card of the related task that was opened when the editor closes', async () => {
      renderTasksPage();
      await openTitle('Implement login');
      fireEvent.click(
        within(cardOf('Implement login')).getByRole('button', { name: 'Open Write the spec' })
      );
      await screen.findByRole('button', { name: 'Rename Write the spec' });

      fireEvent.click(screen.getByRole('button', { name: 'Close' }));

      await waitFor(() => expect(focusedTitle()).toBe('task-spec'));
    });

    describe('with the project filter on', () => {
      beforeEach(() => {
        mockGetTasks.mockImplementation((_workspace: string, project?: string) =>
          Promise.resolve(
            project ? linked.filter((task) => task.project_ids.includes(project)) : linked
          )
        );
      });

      async function filterToAlpha(): Promise<void> {
        renderTasksPage();
        await openTitle('Implement login');
        chooseSelect('Filter by project', 'Project Alpha');
        await waitFor(() =>
          expect(screen.queryByRole('button', { name: 'Write the spec' })).not.toBeInTheDocument()
        );
      }

      it('names the related tasks of other projects', async () => {
        await filterToAlpha();

        const host = within(cardOf('Implement login'));
        await waitFor(() =>
          expect(
            within(host.getByRole('list', { name: 'Depends on' }))
              .getAllByRole('listitem')
              .map((item) => item.textContent)
          ).toEqual(['Write the specCompleted', '1 unknown'])
        );
        expect(
          within(host.getByRole('list', { name: 'Blocks' }))
            .getAllByRole('button')
            .map((button) => button.getAttribute('aria-label'))
        ).toEqual(['Open Ship the release']);
      });

      it('says so when the tasks of other projects cannot be read', async () => {
        mockGetTasks.mockImplementation((_workspace: string, project?: string) =>
          project
            ? Promise.resolve(linked.filter((task) => task.project_ids.includes(project)))
            : Promise.reject(new Error('The workspace could not be read'))
        );
        renderTasksPage();
        await screen.findByRole('alert');
        chooseSelect('Filter by project', 'Project Alpha');

        await waitFor(() =>
          expect(screen.getByRole('alert')).toHaveTextContent(
            'Tasks in other projects could not be loaded'
          )
        );
        expect(screen.getByRole('button', { name: 'Implement login' })).toBeInTheDocument();
      });

      it('returns to the card a filtered-out related task was opened from', async () => {
        await filterToAlpha();
        fireEvent.click(
          await within(cardOf('Implement login')).findByRole('button', {
            name: 'Open Write the spec',
          })
        );
        await screen.findByRole('button', { name: 'Rename Write the spec' });

        fireEvent.click(screen.getByRole('button', { name: 'Close' }));

        await waitFor(() => expect(focusedTitle()).toBe('task-1'));
      });
    });

    it('lists the links in the editor from the tasks the board already loaded', async () => {
      renderTasksPage();
      fireEvent.click(await openTitle('Implement login'));

      const links = within(await editing('Implement login'));
      expect(
        within(links.getByRole('list', { name: 'Depends on' }))
          .getAllByRole('listitem')
          .map((item) => item.textContent)
      ).toEqual(['Write the specCompleted', '1 unknown']);
      expect(
        within(links.getByRole('list', { name: 'Blocks' }))
          .getAllByRole('listitem')
          .map((item) => item.textContent)
      ).toEqual(['Ship the releaseIn review']);
      expect(mockGetTasks).toHaveBeenCalledTimes(1);
    });

    it('shows the pull request and branch in the editor', async () => {
      renderTasksPage('/tasks?task=task-spec');

      const links = within(await editing('Write the spec'));
      expect(links.getByRole('link', { name: PULL_REQUEST_CHIP })).toHaveAttribute(
        'href',
        'https://github.com/test/repo/pull/123'
      );
      expect(links.getByText('feature/fix-button-styling')).toBeInTheDocument();
      expect(links.getByRole('button', { name: 'Open Implement login' })).toBeInTheDocument();
    });

    it('leaves the Links section out of the editor of a task with nothing linked', async () => {
      mockGetTasks.mockImplementation(() => Promise.resolve([mockTasks[0]]));
      mockGetTask.mockImplementation(() => Promise.resolve(mockTasks[0]));
      renderTasksPage('/tasks?task=task-1');

      await screen.findByRole('button', { name: 'Rename Implement login' });
      expect(screen.queryByRole('region', { name: 'Links' })).not.toBeInTheDocument();
    });

    it('switches the editor to a related task and back to its card on close', async () => {
      renderTasksPage();
      fireEvent.click(await openTitle('Implement login'));
      const links = within(await editing('Implement login'));

      fireEvent.click(links.getByRole('button', { name: 'Open Ship the release' }));

      const next = within(await editing('Ship the release'));
      expect(new URLSearchParams(window.location.search).get('task')).toBe('task-release');
      expect(mockGetTask).toHaveBeenCalledWith('task-release');
      expect(screen.queryByRole('button', { name: 'Rename Implement login' })).toBeNull();
      expect(next.getByRole('button', { name: 'Open Implement login' })).toBeInTheDocument();
      expect(mockUpdateTask).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Close' }));
      await waitFor(() => expect(focusedTitle()).toBe('task-release'));
    });

    it('takes focus to the title of the related task the editor switched to', async () => {
      renderTasksPage();
      fireEvent.click(await openTitle('Implement login'));
      const related = within(await editing('Implement login')).getByRole('button', {
        name: 'Open Ship the release',
      });
      related.focus();

      fireEvent.click(related);

      const title = await screen.findByRole('button', { name: 'Rename Ship the release' });
      await waitFor(() => expect(document.activeElement).toBe(title));
    });

    it('leaves focus alone when the editor opens from the board', async () => {
      renderTasksPage();
      fireEvent.click(await openTitle('Implement login'));

      const title = await screen.findByRole('button', { name: 'Rename Implement login' });
      expect(document.activeElement).not.toBe(title);
    });

    it('keeps focus on the related task of a card that a reload moves', async () => {
      let stored = linked;
      mockGetTasks.mockImplementation(() => Promise.resolve(stored));
      renderTasksPage();
      await openTitle('Implement login');
      const host = within(cardOf('Implement login'));
      host.getByRole('button', { name: 'Open Ship the release' }).focus();

      stored = stored.map((task) =>
        task.id === 'task-1' ? { ...task, status: 'in_progress' } : task
      );
      const loads = mockGetTasks.mock.calls.length;
      await act(async () => {
        window.dispatchEvent(new Event('visibilitychange'));
        await waitFor(() => expect(mockGetTasks.mock.calls.length).toBe(loads + 1));
        await mockGetTasks.mock.results[loads].value;
      });
      await waitFor(() => {
        expect(column('In progress').querySelector('[data-task-id="task-1"]')).not.toBeNull();
      });

      expect(document.activeElement).toBe(
        within(column('In progress')).getByRole('button', { name: 'Open Ship the release' })
      );
    });

    describe('a card that folds a relation', () => {
      const plan: Task = { ...mockTasks[0], id: 'task-plan', title: 'Plan the work' };
      const host: Task = {
        ...login,
        dependencies: ['task-spec', 'task-plan'],
        pr_url: 'https://github.com/test/repo/pull/9',
        pr_status: 'open',
      };
      let stored: Task[];

      beforeEach(() => {
        stored = [host, spec, plan, release];
        mockGetTasks.mockImplementation(() => Promise.resolve(stored));
        mockGetTask.mockImplementation((id: string) => {
          const found = stored.find((task) => task.id === id);
          return found ? Promise.resolve(found) : Promise.reject(new Error('Task not found'));
        });
      });

      async function reload(changed: Partial<Task> & Pick<Task, 'id'>): Promise<void> {
        stored = stored.map((task) => (task.id === changed.id ? { ...task, ...changed } : task));
        const loads = mockGetTasks.mock.calls.length;
        await act(async () => {
          window.dispatchEvent(new Event('visibilitychange'));
          await waitFor(() => expect(mockGetTasks.mock.calls.length).toBe(loads + 1));
          await mockGetTasks.mock.results[loads].value;
        });
      }

      function named(): (string | null)[] {
        const list = within(cardOf('Implement login')).getByRole('list', { name: 'Depends on' });
        return within(list)
          .getAllByRole('button', { name: /^Open / })
          .map((button) => button.textContent);
      }

      it('stays expanded after a task it only named once expanded is opened and closed', async () => {
        renderTasksPage();
        await openTitle('Implement login');
        expect(named()).toEqual(['Plan the work']);

        fireEvent.click(within(cardOf('Implement login')).getByRole('button', { name: '+1 more' }));
        expect(named()).toEqual(['Plan the work', 'Write the spec']);
        fireEvent.click(
          within(cardOf('Implement login')).getByRole('button', { name: 'Open Write the spec' })
        );
        await editing('Write the spec');
        fireEvent.click(screen.getByRole('button', { name: 'Close' }));
        await openTitle('Implement login');

        expect(named()).toEqual(['Plan the work', 'Write the spec']);
        const fold = within(cardOf('Implement login')).getByRole('button', { name: 'Show less' });
        expect(fold).toHaveAttribute('aria-expanded', 'true');

        fireEvent.click(fold);
        expect(named()).toEqual(['Plan the work']);
      });

      it('stays expanded on a card that a reload moves, and keeps focus on its count', async () => {
        renderTasksPage();
        await openTitle('Implement login');
        const count = within(cardOf('Implement login')).getByRole('button', { name: '+1 more' });
        fireEvent.click(count);
        count.focus();

        await reload({ id: 'task-1', status: 'in_progress' });
        await waitFor(() => {
          expect(column('In progress').querySelector('[data-task-id="task-1"]')).not.toBeNull();
        });

        expect(named()).toEqual(['Plan the work', 'Write the spec']);
        expect(document.activeElement).toBe(
          within(column('In progress')).getByRole('button', { name: 'Show less' })
        );
      });

      it('keeps focus on the pull request of a card that a reload moves', async () => {
        renderTasksPage();
        await openTitle('Implement login');
        within(cardOf('Implement login'))
          .getByRole('link', { name: 'Pull request #9 open' })
          .focus();

        await reload({ id: 'task-1', status: 'in_progress' });
        await waitFor(() => {
          expect(column('In progress').querySelector('[data-task-id="task-1"]')).not.toBeNull();
        });

        expect(document.activeElement).toBe(
          within(column('In progress')).getByRole('link', { name: 'Pull request #9 open' })
        );
      });

      it('moves focus to the title when the related task that had it is no longer named', async () => {
        renderTasksPage();
        await openTitle('Implement login');
        within(cardOf('Implement login'))
          .getByRole('button', { name: 'Open Plan the work' })
          .focus();

        await reload({ id: 'task-plan', status: 'complete' });
        await waitFor(() => expect(named()).toEqual(['Write the spec']));

        expect(focusedTitle()).toBe('task-1');
      });

      it('moves focus to the title when the count that had it has nothing left to fold', async () => {
        renderTasksPage();
        await openTitle('Implement login');
        const count = within(cardOf('Implement login')).getByRole('button', { name: '+1 more' });
        fireEvent.click(count);
        count.focus();

        await reload({ id: 'task-1', dependencies: ['task-plan'] });
        await waitFor(() => expect(named()).toEqual(['Plan the work']));
        expect(
          within(cardOf('Implement login')).queryByRole('button', { name: /more|Show less/ })
        ).toBeNull();
        expect(focusedTitle()).toBe('task-1');

        await reload({ id: 'task-1', dependencies: ['task-spec', 'task-plan'] });
        await waitFor(() =>
          expect(
            within(cardOf('Implement login')).getByRole('button', { name: '+1 more' })
          ).toHaveAttribute('aria-expanded', 'false')
        );
        expect(named()).toEqual(['Plan the work']);
      });
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

  describe('selecting', () => {
    const refusal = 'Task has an active run or is no longer available';

    const planned: Task[] = ['Draft the spec', 'Write the plan', 'Review the plan'].map(
      (title, index) => ({ ...mockTasks[0], id: `planned-${index + 1}`, title })
    );

    function checkbox(title: string): HTMLElement {
      return screen.getByRole('checkbox', { name: `Select ${title}` });
    }

    function selectionBar(): HTMLElement | null {
      return screen.queryByRole('toolbar', { name: 'Selected tasks' });
    }

    async function check(...titles: string[]): Promise<void> {
      for (const title of titles) {
        fireEvent.click(await screen.findByRole('checkbox', { name: `Select ${title}` }));
      }
    }

    async function editSelection(name: string): Promise<HTMLElement> {
      const bar = await screen.findByRole('toolbar', { name: 'Selected tasks' });
      fireEvent.click(within(bar).getByRole('button', { name: 'Edit' }));
      const dialog = await screen.findByRole('dialog', { name });
      chooseSelect('Status', 'Paused');
      fireEvent.click(within(dialog).getByRole('button', { name: 'Save changes' }));
      return dialog;
    }

    it('counts the cards checked on the board', async () => {
      renderTasksPage();
      expect(selectionBar()).toBeNull();

      await check('Implement login', 'Fix button styling');

      expect(await screen.findByRole('toolbar', { name: 'Selected tasks' })).toHaveTextContent(
        '2 selected'
      );
      expect(checkbox('Implement login')).toBeChecked();
      expect(checkbox('Fix button styling')).toBeChecked();
      expect(document.querySelector('.tasks-body[data-selecting]')).not.toBeNull();
    });

    for (const modifier of ['metaKey', 'ctrlKey'] as const) {
      it(`toggles a card from its title with ${modifier} without opening it`, async () => {
        renderTasksPage();
        const title = await openTitle('Implement login');

        fireEvent.click(title, { [modifier]: true });

        expect(await screen.findByText('1 selected')).toBeInTheDocument();
        expect(checkbox('Implement login')).toBeChecked();

        fireEvent.click(title, { [modifier]: true });

        await waitFor(() => expect(selectionBar()).toBeNull());
        expect(checkbox('Implement login')).not.toBeChecked();
        expect(document.querySelector('section.task-details')).toBeNull();
        expect(new URLSearchParams(window.location.search).get('task')).toBeNull();
      });
    }

    it('selects every card between the last checked and a shift-clicked one in a column', async () => {
      mockGetTasks.mockImplementation(() => Promise.resolve([...planned, mockTasks[1]]));
      renderTasksPage();

      await check('Draft the spec');
      fireEvent.click(checkbox('Review the plan'), { shiftKey: true });

      expect(await screen.findByText('3 selected')).toBeInTheDocument();
      expect(checkbox('Write the plan')).toBeChecked();
      expect(checkbox('Fix button styling')).not.toBeChecked();
    });

    it('clears the selection with Escape', async () => {
      renderTasksPage();
      await check('Implement login', 'Fix button styling');
      await screen.findByText('2 selected');

      fireEvent.keyDown(document.body, { key: 'Escape' });

      await waitFor(() => expect(selectionBar()).toBeNull());
      expect(checkbox('Implement login')).not.toBeChecked();
      expect(document.querySelector('.tasks-body[data-selecting]')).toBeNull();
    });

    it('keeps the selection when Escape cancels a board rename', async () => {
      renderTasksPage();
      await check('Fix button styling');
      await screen.findByText('1 selected');

      await chooseMenuItem('Actions for Implement login', 'Rename');
      fireEvent.keyDown(screen.getByRole('textbox', { name: 'Task title' }), { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByRole('textbox', { name: 'Task title' })).not.toBeInTheDocument();
      });
      expect(screen.getByText('1 selected')).toBeInTheDocument();
    });

    it('keeps the selection when Escape puts back a card picked up by keyboard', async () => {
      const restore = layOutBoard();
      try {
        renderTasksPage();
        await check('Fix button styling');
        await screen.findByText('1 selected');

        const handle = await screen.findByRole('button', { name: 'Move Implement login' });
        handle.focus();
        await pressKey(handle, 'Space');
        expect(document.querySelector('.board[data-dragging]')).not.toBeNull();

        await pressKey(handle, 'Escape');

        expect(document.querySelector('.board[data-dragging]')).toBeNull();
        expect(screen.getByText('1 selected')).toBeInTheDocument();
        expect(checkbox('Fix button styling')).toBeChecked();
        expect(mockUpdateTask).not.toHaveBeenCalled();
      } finally {
        restore();
      }
    });

    it('clears the selection when the project filter changes', async () => {
      renderTasksPage();
      await check('Implement login');
      await screen.findByText('1 selected');

      chooseSelect('Filter by project', 'Project Alpha');

      await waitFor(() => expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-1'));
      expect(
        await screen.findByRole('checkbox', { name: 'Select Implement login' })
      ).not.toBeChecked();
      expect(selectionBar()).toBeNull();
    });

    it('clears the selection when the workspace changes', async () => {
      const { again } = renderTasksPage();
      await check('Implement login');
      await screen.findByText('1 selected');

      currentWorkspace = { ...mockWorkspace, id: 'workspace-2', name: 'Other Workspace' };
      again();

      await waitFor(() => expect(mockGetTasks).toHaveBeenCalledWith('workspace-2', undefined));
      expect(
        await screen.findByRole('checkbox', { name: 'Select Implement login' })
      ).not.toBeChecked();
      expect(selectionBar()).toBeNull();
    });

    it('drops a task that leaves the listing from the selection', async () => {
      let listed = mockTasks;
      mockGetTasks.mockImplementation(() => Promise.resolve(listed));
      renderTasksPage();
      await check('Implement login', 'Fix button styling');
      await screen.findByText('2 selected');

      listed = [mockTasks[0]];
      const loads = mockGetTasks.mock.calls.length;
      await act(async () => {
        window.dispatchEvent(new Event('visibilitychange'));
        await waitFor(() => expect(mockGetTasks.mock.calls.length).toBe(loads + 1));
      });

      await waitFor(() => expect(screen.queryByText('Fix button styling')).toBeNull());
      expect(screen.getByText('1 selected')).toBeInTheDocument();
    });

    it('edits the selected tasks together, then clears the selection', async () => {
      mockUpdateTasks.mockImplementation(
        (_workspace: string, ids: readonly string[], changes: Partial<Task>) =>
          Promise.resolve(
            mockTasks
              .filter((task) => ids.includes(task.id))
              .map((task) => ({ ...task, ...changes }))
          )
      );
      renderTasksPage();
      await check('Implement login', 'Fix button styling');

      await editSelection('Edit 2 tasks');

      await waitFor(() => {
        expect(mockUpdateTasks).toHaveBeenCalledWith('workspace-1', ['task-1', 'task-2'], {
          status: 'paused',
        });
      });
      await waitFor(() => expect(mockToastSuccess).toHaveBeenCalledWith('Updated 2 tasks'));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(selectionBar()).toBeNull();
      expect(checkbox('Implement login')).not.toBeChecked();
      expect(mockUpdateTasks).toHaveBeenCalledTimes(1);
      expect(mockToastError).not.toHaveBeenCalled();
    });

    it('keeps the dialog and the selection open when the edit is refused', async () => {
      mockUpdateTasks.mockImplementation(() => Promise.reject(new Error(refusal)));
      renderTasksPage();
      await check('Implement login');

      const dialog = await editSelection('Edit 1 task');

      await waitFor(() => {
        expect(mockToastError).toHaveBeenCalledWith("Couldn't update 1 task", {
          description: refusal,
        });
      });
      await waitFor(() => {
        expect(within(dialog).getByRole('button', { name: 'Save changes' })).toBeEnabled();
      });
      expect(screen.getByRole('dialog', { name: 'Edit 1 task' })).toBeInTheDocument();
      expect(screen.getByText('1 selected')).toBeInTheDocument();
      expect(mockToastSuccess).not.toHaveBeenCalled();
    });
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
