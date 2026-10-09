import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { SetupPlan } from '../../../api/models';
import { setupCompleteKey } from '../setupComplete';

const mockGetSetup = mock();
const mockStartSetup = mock();
const mockNavigate = mock();
const mockPull = mock(() => Promise.resolve(true));

mock.module('../../../api/models', () => ({
  modelsApi: {
    getSetup: mockGetSetup,
    startSetup: mockStartSetup,
  },
  SetupError: class SetupError extends Error {
    code: string;
    plan: SetupPlan;
    constructor(refusal: { error: string; code: string; plan: SetupPlan }) {
      super(refusal.error);
      this.code = refusal.code;
      this.plan = refusal.plan;
    }
  },
}));

const mockGetHostMounts = mock();
const mockGetWorkspaceHostDirectories = mock();
const mockUpdateWorkspaceHostDirectories = mock();

mock.module('../../../api/client', () => ({
  client: {
    getHostMounts: mockGetHostMounts,
    getWorkspaceHostDirectories: mockGetWorkspaceHostDirectories,
    updateWorkspaceHostDirectories: mockUpdateWorkspaceHostDirectories,
  },
}));

mock.module('../../auth', () => ({
  useAuth: () => ({
    user: { id: 'user-1' },
  }),
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentOrganization: { id: 'org-1' },
    currentWorkspace: { id: 'ws-1' },
  }),
}));

mock.module('react-router-dom', () => ({
  useNavigate: () => mockNavigate,
}));

const pull = {
  jobs: [] as Array<{
    id: string;
    modelName: string;
    pulling: boolean;
    progress: number | null;
    steps: Array<{ name: string; message: string; status: 'pending' | 'success' | 'error' }>;
    result: { success: boolean; message: string } | null;
  }>,
  pulling: false,
  activeCount: 0,
  progress: null,
  chunk: null,
  steps: [],
  result: null,
  model: null,
  minimized: false,
  setMinimized: mock(),
  canStart: () => true,
  pull: mockPull,
  reset: mock(),
  cancel: mock(),
  dismiss: mock(),
};

mock.module('../hooks/usePull', () => ({
  usePull: () => pull,
}));

function plan(overrides: Partial<SetupPlan> = {}): SetupPlan {
  return {
    ram_bytes: 64 * 1024 ** 3,
    ram_label: '64 GB',
    disk_free_bytes: 500_000_000_000,
    disk_free_label: '500 GB',
    vision_min_ram_bytes: 16 * 1024 ** 3,
    disk_margin_bytes: 10_000_000_000,
    chat_preset: '32gb',
    recommended_preset: '32gb',
    chat_presets: [
      {
        id: '32gb',
        label: '32 GB+ RAM',
        min_ram_bytes: 32 * 1024 ** 3,
        fast: 'llama3.1:8b',
        reason: 'deepseek-r1:32b',
      },
    ],
    features: [
      {
        id: 'chat',
        label: 'Chat',
        description: 'Fast, reasoning, and embedding models',
        required: true,
        selected: true,
        blocked: false,
        block_reason: null,
        size_bytes: 25_600_000_000,
        size_label: '26 GB',
        present_bytes: 0,
        needed_bytes: 25_600_000_000,
        ready: false,
        licenses: [],
      },
      {
        id: 'vision',
        label: 'Vision',
        description: 'Image attachments',
        required: false,
        selected: true,
        blocked: false,
        block_reason: null,
        size_bytes: 4_700_000_000,
        size_label: '4.7 GB',
        present_bytes: 0,
        needed_bytes: 4_700_000_000,
        ready: false,
        licenses: [],
      },
    ],
    wants_all: true,
    gate: null,
    totals: {
      size_bytes: 30_300_000_000,
      size_label: '30 GB',
      present_bytes: 0,
      present_label: '0 B',
      needed_bytes: 30_300_000_000,
      needed_label: '30 GB',
      working_space_bytes: 10_000_000_000,
      working_space_label: '10 GB',
      required_free_bytes: 40_300_000_000,
      required_free_label: '40 GB',
      free_now_bytes: 500_000_000_000,
      free_now_label: '500 GB',
      short_by_bytes: 0,
      short_by_label: null,
    },
    licenses: [],
    artifacts: [],
    pulls: [
      { model: 'llama3.1:8b', runtime: 'ollama' },
      { model: 'llava:7b', runtime: 'ollama' },
    ],
    ...overrides,
  };
}

let SetupPage: typeof import('./SetupPage').default;

beforeAll(async () => {
  SetupPage = (await import('./SetupPage')).default;
});

afterAll(() => {
  mock.restore();
});

describe('SetupPage', () => {
  beforeEach(() => {
    localStorage.clear();
    mockGetSetup.mockReset();
    mockStartSetup.mockReset();
    mockNavigate.mockReset();
    mockPull.mockReset();
    mockPull.mockImplementation(() => Promise.resolve(true));
    pull.jobs = [];
    mockGetSetup.mockResolvedValue(plan());
    mockGetHostMounts.mockReset();
    mockGetWorkspaceHostDirectories.mockReset();
    mockUpdateWorkspaceHostDirectories.mockReset();
    mockGetHostMounts.mockResolvedValue({
      in_container: true,
      host_root: '/Users/jake/Local',
      container_root: '/host',
      ready: true,
      hint: 'Folders must live under /Users/jake/Local.',
    });
    mockGetWorkspaceHostDirectories.mockResolvedValue({ directories: [], folders: [] });
    mockUpdateWorkspaceHostDirectories.mockResolvedValue({ directories: [], folders: [] });
  });

  it('lets the user pick features and skip into the app', async () => {
    render(<SetupPage />);
    expect(await screen.findByRole('heading', { name: 'Set up Zone' })).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'Chat' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'Vision' })).toBeChecked();
    expect(screen.getByText('Free required')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install selected' })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: 'Skip for now' }));
    expect(await screen.findByRole('heading', { name: 'Host folders' })).toBeInTheDocument();
    expect(mockNavigate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Skip for now' }));
    expect(localStorage.getItem(setupCompleteKey('user-1'))).toBe('1');
    expect(mockNavigate).toHaveBeenCalledWith('/', { replace: true });
  });

  it('shows per-piece download progress on the setup step', async () => {
    pull.jobs = [
      {
        id: '1',
        modelName: 'llama3.1:8b',
        pulling: true,
        progress: 40,
        steps: [],
        result: null,
      },
    ];
    render(<SetupPage />);
    expect(await screen.findByRole('heading', { name: 'Downloads' })).toBeInTheDocument();
    expect(screen.getByText('llama3.1:8b')).toBeInTheDocument();
    expect(screen.getByText('40%')).toBeInTheDocument();
  });

  it('continues after selected features finish downloading', async () => {
    mockStartSetup.mockResolvedValue(plan({ pulls: [] }));
    mockGetSetup.mockResolvedValue(plan({ pulls: [] }));
    render(<SetupPage />);
    expect(await screen.findByRole('button', { name: 'Continue' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await screen.findByRole('heading', { name: 'Host folders' })).toBeInTheDocument();
    expect(mockNavigate).not.toHaveBeenCalled();
  });

  it('starts downloads from install selected', async () => {
    const started = plan();
    mockStartSetup.mockResolvedValue(started);
    render(<SetupPage />);
    await screen.findByRole('button', { name: 'Install selected' });
    fireEvent.click(screen.getByRole('button', { name: 'Install selected' }));
    await waitFor(() => expect(mockStartSetup).toHaveBeenCalled());
    await waitFor(() => expect(mockPull).toHaveBeenCalledTimes(2));
  });
});
