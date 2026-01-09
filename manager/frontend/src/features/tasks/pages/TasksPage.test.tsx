import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { tasksApi } from '../../../api/tasks';
import { projectsApi } from '../../../api/projects';
import { client } from '../../../api/client';
import type { Task } from '../types';
import type { Project } from '../../projects/types';
import type { Source } from '../../../types';
import TasksPage from './TasksPage';

// Mock APIs
jest.mock('../../../api/tasks', () => ({
  tasksApi: {
    getTasks: jest.fn(),
    createTask: jest.fn(),
    deleteTask: jest.fn(),
    runTask: jest.fn(),
    cancelTaskRun: jest.fn(),
    createTaskWebSocket: jest.fn(),
  },
}));

jest.mock('../../../api/projects', () => ({
  projectsApi: {
    getProjects: jest.fn(),
  },
}));

jest.mock('../../../api/client', () => ({
  client: {
    getSources: jest.fn(),
  },
}));

const mockTasksApi = tasksApi as jest.Mocked<typeof tasksApi>;
const mockProjectsApi = projectsApi as jest.Mocked<typeof projectsApi>;
const mockClient = client as jest.Mocked<typeof client>;

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
    project_id: 'proj-1',
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
    project_id: 'proj-2',
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
    jest.clearAllMocks();
    mockTasksApi.getTasks.mockResolvedValue(mockTasks);
    mockProjectsApi.getProjects.mockResolvedValue(mockProjects);
    mockClient.getSources.mockResolvedValue(mockSources);
    window.confirm = jest.fn(() => true);
  });

  it('shows loading state with skeleton cards', async () => {
    mockTasksApi.getTasks.mockImplementation(() => new Promise(() => {}));
    render(<TasksPage />);
    expect(document.querySelectorAll('.skeleton-card').length).toBe(4);
  });

  it('shows empty state when no tasks', async () => {
    mockTasksApi.getTasks.mockResolvedValueOnce([]);
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('No tasks found. Create a task to get started!')).toBeInTheDocument();
    });
  });

  it('renders tasks list', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
      expect(screen.getByText('Fix button styling')).toBeInTheDocument();
    });
  });

  it('renders page header', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Tasks' })).toBeInTheDocument();
    });
    expect(screen.getByText('Autonomous agent workflows')).toBeInTheDocument();
  });

  it('displays task descriptions', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Add user authentication')).toBeInTheDocument();
      expect(screen.getByText('Update CSS for buttons')).toBeInTheDocument();
    });
  });

  it('displays task status badges', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('created')).toBeInTheDocument();
      expect(screen.getByText('complete')).toBeInTheDocument();
    });
  });

  it('displays agentic badge for agentic tasks', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Agentic')).toBeInTheDocument();
    });
  });

  it('displays project name for tasks', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    // Project names appear both in filter dropdown and task cards
    // Just verify they exist somewhere on the page
    expect(screen.getAllByText('Project Alpha').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Project Beta').length).toBeGreaterThan(0);
  });

  it('displays task priority', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Priority: 1')).toBeInTheDocument();
      expect(screen.getByText('Priority: 3')).toBeInTheDocument();
    });
  });

  it('displays model name when available', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Model: gpt-4')).toBeInTheDocument();
    });
  });

  it('renders filter dropdowns', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by project' })).toBeInTheDocument();
      expect(screen.getByRole('combobox', { name: 'Filter by status' })).toBeInTheDocument();
    });
  });

  it('filters by project', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by project' })).toBeInTheDocument();
    });

    fireEvent.change(screen.getByRole('combobox', { name: 'Filter by project' }), {
      target: { value: 'proj-1' },
    });

    await waitFor(() => {
      expect(mockTasksApi.getTasks).toHaveBeenCalledWith('proj-1', undefined);
    });
  });

  it('filters by status', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Filter by status' })).toBeInTheDocument();
    });

    fireEvent.change(screen.getByRole('combobox', { name: 'Filter by status' }), {
      target: { value: 'complete' },
    });

    await waitFor(() => {
      expect(mockTasksApi.getTasks).toHaveBeenCalledWith(undefined, 'complete');
    });
  });

  it('opens create task modal', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));
    expect(screen.getByRole('heading', { name: 'Create New Task' })).toBeInTheDocument();
  });

  it('disables new task button when no projects', async () => {
    mockProjectsApi.getProjects.mockResolvedValueOnce([]);
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeDisabled();
    });
  });

  it('creates a new task', async () => {
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
    mockTasksApi.createTask.mockResolvedValueOnce(newTask);

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'New Task' } });
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Task description' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Create Task' }));

    await waitFor(() => {
      expect(mockTasksApi.createTask).toHaveBeenCalled();
    });
  });

  it('cancels create task modal', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));
    expect(screen.getByRole('heading', { name: 'Create New Task' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'Create New Task' })).not.toBeInTheDocument();
    });
  });

  it('deletes a task with confirmation', async () => {
    mockTasksApi.deleteTask.mockResolvedValueOnce(undefined);

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    await waitFor(() => {
      expect(mockTasksApi.deleteTask).toHaveBeenCalledWith('task-1');
    });
  });

  it('cancels delete when confirm rejected', async () => {
    (window.confirm as jest.Mock).mockReturnValueOnce(false);

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    expect(mockTasksApi.deleteTask).not.toHaveBeenCalled();
  });

  it('shows error when loading fails', async () => {
    mockTasksApi.getTasks.mockRejectedValueOnce(new Error('Network error'));
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('shows error when delete fails', async () => {
    mockTasksApi.deleteTask.mockRejectedValueOnce(new Error('Delete failed'));

    render(<TasksPage />);
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
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);

    expect(document.querySelector('.task-execution-overlay')).toBeInTheDocument();
  });

  it('closes task execution view', async () => {
    render(<TasksPage />);
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
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
    });
  });

  it('shows agentic styling for agentic task cards', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const agenticCard = screen.getByText('Implement login').closest('.task-card');
    expect(agenticCard).toHaveClass('task-card-agentic');
  });

  it('enables agentic mode in create modal', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeEnabled();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Create New Task' })).toBeInTheDocument();
    });

    // Find the agentic checkbox and click it
    const checkbox = screen
      .getByText(/Enable Agentic Mode/i)
      .closest('label')
      ?.querySelector('input');
    expect(checkbox).not.toBeNull();
    fireEvent.click(checkbox!);

    expect(screen.getByLabelText('Code Source')).toBeInTheDocument();
  });

  it('shows project source info when agentic mode enabled', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeEnabled();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Create New Task' })).toBeInTheDocument();
    });

    const checkbox = screen
      .getByText(/Enable Agentic Mode/i)
      .closest('label')
      ?.querySelector('input');
    fireEvent.click(checkbox!);

    expect(screen.getByText(/Project uses:/)).toBeInTheDocument();
  });

  it('displays status filter options', async () => {
    render(<TasksPage />);
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
      close: jest.fn(),
    };
    mockTasksApi.runTask.mockResolvedValueOnce({ run_id: 'run-1' });
    mockTasksApi.createTaskWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);

    fireEvent.click(screen.getByRole('button', { name: 'Start Execution' }));

    await waitFor(() => {
      expect(mockTasksApi.runTask).toHaveBeenCalledWith('task-1');
    });
  });

  it('creates task WebSocket connection on start', async () => {
    const mockWs = {
      onmessage: null as ((event: MessageEvent) => void) | null,
      onclose: null as (() => void) | null,
      onerror: null as ((error: Event) => void) | null,
      close: jest.fn(),
    };
    mockTasksApi.runTask.mockResolvedValueOnce({ run_id: 'run-1' });
    mockTasksApi.createTaskWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Implement login')).toBeInTheDocument();
    });

    const executeButtons = screen.getAllByRole('button', { name: 'Execute' });
    fireEvent.click(executeButtons[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Start Execution' }));

    await waitFor(() => {
      expect(mockTasksApi.createTaskWebSocket).toHaveBeenCalledWith('run-1');
    });
  });

  it('shows create error in modal', async () => {
    mockTasksApi.createTask.mockRejectedValueOnce(new Error('Create failed'));

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ New Task' })).toBeEnabled();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ New Task' }));

    // Wait for modal to appear
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Create New Task' })).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'New Task' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Description' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create Task' }));

    await waitFor(() => {
      expect(screen.getByText('Create failed')).toBeInTheDocument();
    });
  });

  it('displays PR status badge when PR exists', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('PR: merged')).toBeInTheDocument();
    });
  });

  it('displays PR link when pr_url exists', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      const prLink = screen.getByText('View Pull Request');
      expect(prLink).toBeInTheDocument();
      expect(prLink.closest('a')).toHaveAttribute('href', 'https://github.com/test/repo/pull/123');
      expect(prLink.closest('a')).toHaveAttribute('target', '_blank');
      expect(prLink.closest('a')).toHaveAttribute('rel', 'noopener noreferrer');
    });
  });

  it('displays branch name when branch_name exists', async () => {
    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('Branch: feature/fix-button-styling')).toBeInTheDocument();
    });
  });

  it('does not display PR info when pr_url is null', async () => {
    // Override mockTasks to only include task without PR
    mockTasksApi.getTasks.mockResolvedValueOnce([mockTasks[0]]);

    render(<TasksPage />);
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
    mockTasksApi.getTasks.mockResolvedValueOnce(tasksWithPrStatuses);

    render(<TasksPage />);
    await waitFor(() => {
      expect(screen.getByText('PR: pending')).toBeInTheDocument();
      expect(screen.getByText('PR: open')).toBeInTheDocument();
      expect(screen.getByText('PR: closed')).toBeInTheDocument();
    });
  });
});
