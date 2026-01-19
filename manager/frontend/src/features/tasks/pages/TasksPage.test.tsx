import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, mock, beforeEach, describe, it, expect } from 'bun:test';
import type { Task } from '../types';
import type { Project } from '../../projects/types';
import type { Source } from '../../../types';
import TasksPage from './TasksPage';

// Create mock functions for tasks API
const mockGetTasks = mock(() => Promise.resolve([] as Task[]));
const mockCreateTask = mock(() => Promise.resolve({} as Task));
const mockDeleteTask = mock(() => Promise.resolve());
const mockRunTask = mock(() => Promise.resolve({ run_id: 'run-1' }));
const mockCancelTaskRun = mock(() => Promise.resolve());
const mockCreateTaskWebSocket = mock(() => ({
  onmessage: null,
  onclose: null,
  onerror: null,
  close: mock(() => {}),
}));

// Create mock functions for projects API
const mockGetProjects = mock(() => Promise.resolve([] as Project[]));

// Create mock functions for client
const mockGetSources = mock(() => Promise.resolve([] as Source[]));

// Mock workspace context
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

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: mock(() => ({
    currentWorkspace: mockWorkspace,
    loading: false,
  })),
}));

// Mock APIs
mock.module('../../../api/tasks', () => ({
  tasksApi: {
    getTasks: mockGetTasks,
    createTask: mockCreateTask,
    deleteTask: mockDeleteTask,
    runTask: mockRunTask,
    cancelTaskRun: mockCancelTaskRun,
    createTaskWebSocket: mockCreateTaskWebSocket,
  },
}));

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProjects: mockGetProjects,
  },
}));

mock.module('../../../api/client', () => ({
  client: {
    getSources: mockGetSources,
  },
}));

afterAll(() => {
  mock.restore();
});

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });

const renderTasksPage = () => {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <TasksPage />
    </QueryClientProvider>
  );
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

describe('TasksPage', () => {
  beforeEach(() => {
    mockGetTasks.mockReset();
    mockCreateTask.mockReset();
    mockDeleteTask.mockReset();
    mockRunTask.mockReset();
    mockGetProjects.mockReset();
    mockGetSources.mockReset();
    mockGetTasks.mockImplementation(() => Promise.resolve(mockTasks));
    mockGetProjects.mockImplementation(() => Promise.resolve(mockProjects));
    mockGetSources.mockImplementation(() => Promise.resolve(mockSources));
    window.confirm = mock(() => true);
  });

  it('shows loading state with skeleton cards', async () => {
    mockGetTasks.mockImplementation(() => new Promise(() => {}));
    renderTasksPage();
    expect(document.querySelectorAll('.skeleton-card').length).toBe(4);
  });

  it('shows empty state when no tasks', async () => {
    mockGetTasks.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('No tasks yet')).toBeInTheDocument();
    });
  });

  it('renders tasks list', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
      expect(screen.getByText('Fix button styling')).toBeInTheDocument();
    });
  });

  it('renders page header', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(screen.getByText('Autonomous agent workflows')).toBeInTheDocument();
  });

  it('displays task descriptions', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Add user authentication')).toBeInTheDocument();
      expect(screen.getByText('Update CSS for buttons')).toBeInTheDocument();
    });
  });

  it('displays task status badges', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('created')).toBeInTheDocument();
      expect(screen.getByText('complete')).toBeInTheDocument();
    });
  });

  it('displays agentic badge for agentic tasks', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Agentic')).toBeInTheDocument();
    });
  });

  it('displays project name for tasks', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    // Project names appear both in filter dropdown and task cards
    // Just verify they exist somewhere on the page
    expect(screen.getAllByText('Project Alpha').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Project Beta').length).toBeGreaterThan(0);
  });

  it('displays task priority', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Priority: 1')).toBeInTheDocument();
      expect(screen.getByText('Priority: 3')).toBeInTheDocument();
    });
  });

  it('displays model name when available', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Model: gpt-4')).toBeInTheDocument();
    });
  });

  it('renders filter dropdowns', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by project' })).toBeInTheDocument();
      expect(screen.getByRole('combobox', { name: 'Filter by status' })).toBeInTheDocument();
    });
  });

  it('filters by project', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by project' })).toBeInTheDocument();
    });

    fireEvent.change(screen.getByRole('combobox', { name: 'Filter by project' }), {
      target: { value: 'proj-1' },
    });

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', 'proj-1', undefined);
    });
  });

  it('filters by status', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by status' })).toBeInTheDocument();
    });

    fireEvent.change(screen.getByRole('combobox', { name: 'Filter by status' }), {
      target: { value: 'complete' },
    });

    await waitFor(() => {
      expect(mockGetTasks).toHaveBeenCalledWith('workspace-1', undefined, 'complete');
    });
  });

  it('opens create task wizard', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));
    expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
  });

  it('disables new task button when no projects', async () => {
    mockGetProjects.mockImplementation(() => Promise.resolve([]));
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeDisabled();
    });
  });

  it('creates a new task via wizard', async () => {
    const newTask: Task = {
      id: 'task-3',
      project_id: 'proj-1',
      title: 'New Task',
      description: 'Task description',
      acceptance_criteria: null,
      status: 'created',
      priority: 2,
      is_agentic: false,
      github_repo_url: null,
      source_id: null,
      source_ids: [],
      model_name: null,
      dependencies: [],
      started_at: null,
      completed_at: null,
      queued_at: null,
      worker_id: null,
      pr_url: null,
      branch_name: null,
      pr_status: null,
      pr_created_at: null,
      created_at: '2024-01-13T00:00:00Z',
      updated_at: '2024-01-13T00:00:00Z',
    };
    mockCreateTask.mockImplementation(() => Promise.resolve(newTask));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));

    // Step 1: Project selection
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 2: Task details
    await waitFor(() => {
      expect(screen.getByLabelText('Title')).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'New Task' } });
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Task description' },
    });

    // Go to step 3 (settings)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 3: Submit
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
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));
    expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'New Task' })).not.toBeInTheDocument();
    });
  });

  it('deletes a task with confirmation', async () => {
    mockDeleteTask.mockImplementation(() => Promise.resolve(undefined));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    await waitFor(() => {
      expect(mockDeleteTask).toHaveBeenCalledWith('task-1');
    });
  });

  it('cancels delete when confirm rejected', async () => {
    const confirmMock = window.confirm as unknown as ReturnType<typeof mock>;
    confirmMock.mockReturnValueOnce(false);

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    expect(mockDeleteTask).not.toHaveBeenCalled();
  });

  it('shows error when loading fails', async () => {
    mockGetTasks.mockImplementation(() => Promise.reject(new Error('Network error')));
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('shows error when delete fails', async () => {
    mockDeleteTask.mockImplementation(() => Promise.reject(new Error('Delete failed')));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    await waitFor(() => {
      expect(screen.getByText('Delete failed')).toBeInTheDocument();
    });
  });

  it('opens task execution view', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);

    expect(document.querySelector('.task-execution-overlay')).toBeInTheDocument();
  });

  it('closes task execution view', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);

    expect(document.querySelector('.task-execution-overlay')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(document.querySelector('.task-execution-overlay')).not.toBeInTheDocument();
    });
  });

  it('displays source name for agentic tasks', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
    });
  });

  it('shows agentic styling for agentic task cards', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const agenticCard = screen.getByText('Implement login').closest('.task-card');
    expect(agenticCard).toHaveClass('task-card-agentic');
  });

  it('enables agentic mode in create wizard', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));

    // Step 1: Select project, go to step 2
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 2: Fill in details
    await waitFor(() => {
      expect(screen.getByLabelText('Title')).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'Test Task' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Test description' } });

    // Go to step 3 (settings)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 3: Find the agentic checkbox and click it
    const checkbox = await screen.findByRole('checkbox', { name: /Enable Agentic Mode/i });
    fireEvent.click(checkbox);

    expect(screen.getByLabelText('Code Source')).toBeInTheDocument();
  });

  it('shows project source info when agentic mode enabled', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));

    // Step 1: Select project, go to step 2
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 2: Fill in details
    await waitFor(() => {
      expect(screen.getByLabelText('Title')).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'Test Task' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Test description' } });

    // Go to step 3 (settings)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 3: Enable agentic mode
    const checkbox = await screen.findByRole('checkbox', { name: /Enable Agentic Mode/i });
    fireEvent.click(checkbox);

    expect(screen.getByText(/Project uses:/)).toBeInTheDocument();
  });

  it('displays status filter options', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by status' })).toBeInTheDocument();
    });

    const statusSelect = screen.getByRole('combobox', { name: 'Filter by status' });
    expect(statusSelect).toContainHTML('All Statuses');
    expect(statusSelect).toContainHTML('Created');
    expect(statusSelect).toContainHTML('In Progress');
    expect(statusSelect).toContainHTML('Complete');
  });

  it('starts task execution', async () => {
    const mockWs = {
      onmessage: null as ((event: MessageEvent) => void) | null,
      onclose: null as (() => void) | null,
      onerror: null as ((error: Event) => void) | null,
      close: mock(() => {}),
    };
    mockRunTask.mockImplementation(() => Promise.resolve({ run_id: 'run-1' }));
    mockCreateTaskWebSocket.mockImplementation(() => mockWs as unknown as WebSocket);

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);

    fireEvent.click(screen.getByRole('button', { name: 'Start Execution' }));

    await waitFor(() => {
      expect(mockRunTask).toHaveBeenCalledWith('task-1');
    });
  });

  it('creates task WebSocket connection on start', async () => {
    const mockWs = {
      onmessage: null as ((event: MessageEvent) => void) | null,
      onclose: null as (() => void) | null,
      onerror: null as ((error: Event) => void) | null,
      close: mock(() => {}),
    };
    mockRunTask.mockImplementation(() => Promise.resolve({ run_id: 'run-1' }));
    mockCreateTaskWebSocket.mockImplementation(() => mockWs as unknown as WebSocket);

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Start Execution' }));

    await waitFor(() => {
      expect(mockCreateTaskWebSocket).toHaveBeenCalledWith('run-1');
    });
  });

  it('shows create error in wizard', async () => {
    mockCreateTask.mockImplementation(() => Promise.reject(new Error('Create failed')));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).not.toBeDisabled();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    // Wait for wizard to appear
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New Task' })).toBeInTheDocument();
    });

    // Step 1: Select project
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Project Alpha/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /Project Alpha/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 2: Fill in details
    await waitFor(() => {
      expect(screen.getByLabelText('Title')).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'New Task' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Description' } });

    // Go to step 3
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 3: Submit
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
      expect(screen.getByText('PR: merged')).toBeInTheDocument();
    });
  });

  it('displays PR link when pr_url exists', async () => {
    renderTasksPage();
    await waitFor(() => {
      const prLink = screen.getByText('View Pull Request');
      expect(prLink).toBeInTheDocument();
      expect(prLink.closest('a')).toHaveAttribute('href', 'https://github.com/test/repo/pull/123');
      expect(prLink.closest('a')).toHaveAttribute('target', '_blank');
      expect(prLink.closest('a')).toHaveAttribute('rel', 'noopener noreferrer');
    });
  });

  it('displays branch name when branch_name exists', async () => {
    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Branch: feature/fix-button-styling')).toBeInTheDocument();
    });
  });

  it('does not display PR info when pr_url is null', async () => {
    // Override mockTasks to only include task without PR
    mockGetTasks.mockImplementation(() => Promise.resolve([mockTasks[0]]));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });
    expect(screen.queryByText('View Pull Request')).not.toBeInTheDocument();
  });

  it('displays correct PR badge colors', async () => {
    const tasksWithPrStatuses: Task[] = [
      {
        ...mockTasks[0],
        id: 'task-pending',
        pr_status: 'pending',
      },
      {
        ...mockTasks[0],
        id: 'task-open',
        pr_status: 'open',
      },
      {
        ...mockTasks[0],
        id: 'task-closed',
        pr_status: 'closed',
      },
    ];
    mockGetTasks.mockImplementation(() => Promise.resolve(tasksWithPrStatuses));

    renderTasksPage();
    await waitFor(() => {
      expect(screen.getByText('PR: pending')).toBeInTheDocument();
      expect(screen.getByText('PR: open')).toBeInTheDocument();
      expect(screen.getByText('PR: closed')).toBeInTheDocument();
    });
  });
});
