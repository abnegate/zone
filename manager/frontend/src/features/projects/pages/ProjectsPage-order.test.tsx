import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  spyOn,
} from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type React from 'react';
import { MemoryRouter } from 'react-router-dom';
import { clientModule } from '../../../test/client';
import { chooseSelect } from '../../../test/select';
import type { Project } from '../types';
import { applyOrder, sortKey } from '../utils/order';

const SLOT_HEIGHT = 72;

let userId = 'user-1';
let role = 'member';

let stored: Project[] = [];
const mockGetProjects = mock(() => Promise.resolve(stored));
const mockReorderProjects = mock((_workspace: string, order: readonly string[]) => {
  stored = applyOrder(stored, order);
  return Promise.resolve();
});

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getProjects: mockGetProjects,
    reorderProjects: mockReorderProjects,
    createProject: mock(),
    updateProject: mock(),
    deleteProject: mock(),
    getSyncConfigs: mock(() => Promise.resolve([])),
    createSyncConfig: mock(),
    deleteSyncConfig: mock(),
    startAutoProject: mock(),
    getAutomation: mock(),
    resumeAutomation: mock(),
  },
}));

const mockGetWorkspaceMembers = mock(() =>
  Promise.resolve({ members: [{ user_id: userId, role }] })
);

mock.module('../../../api/client', () =>
  clientModule({
    getSources: mock(() => Promise.resolve([])),
    getWorkspaceMembers: mockGetWorkspaceMembers,
  })
);

mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: userId, email: 'test@test.com' },
    roles: ['user'],
    permissions: [],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
  ResendVerificationButton: () => null,
  VerificationPendingBanner: () => null,
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'workspace-1', name: 'Test Workspace' },
    loading: false,
  }),
}));

let ProjectsPage: typeof import('./ProjectsPage').default;

beforeAll(async () => {
  ProjectsPage = (await import('./ProjectsPage')).default;
});

afterAll(() => {
  mock.restore();
});

function project(id: string, name: string, overrides: Partial<Project>): Project {
  return {
    id,
    name,
    description: null,
    status: 'active',
    github_repo_url: null,
    source_id: null,
    auto: false,
    position: 0,
    last_used_at: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

const PROJECTS: Project[] = [
  project('cloud', 'Cloud', {
    position: 1,
    created_at: '2026-11-06T00:00:00Z',
    last_used_at: '2026-11-01T00:00:00Z',
  }),
  project('jbs', 'JBS', {
    position: 2,
    status: 'on_hold',
    created_at: '2026-10-01T00:00:00Z',
    last_used_at: '2026-11-05T00:00:00Z',
  }),
  project('zone', 'Zone', {
    position: 3,
    created_at: '2026-09-01T00:00:00Z',
    last_used_at: null,
  }),
];

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return render(
    <MemoryRouter initialEntries={['/projects']}>
      <QueryClientProvider client={queryClient}>
        <ProjectsPage />
      </QueryClientProvider>
    </MemoryRouter>
  );
}

function shownOrder(): string[] {
  return Array.from(document.querySelectorAll<HTMLElement>('.project-slot')).map(
    (slot) => slot.dataset.project ?? ''
  );
}

async function loaded() {
  await screen.findByText('Zone');
}

describe('project order', () => {
  beforeEach(() => {
    localStorage.clear();
    userId = 'user-1';
    role = 'member';
    stored = PROJECTS.map((item) => ({ ...item }));
    mockGetProjects.mockClear();
    mockReorderProjects.mockClear();
  });

  it('lists projects in the custom order until another sort is chosen', async () => {
    renderPage();
    await loaded();

    expect(shownOrder()).toEqual(['cloud', 'jbs', 'zone']);
    expect(screen.getByRole('combobox', { name: 'Sort projects' })).toHaveTextContent('Custom');
  });

  it('sorts by the chosen option and offers every one', async () => {
    renderPage();
    await loaded();

    chooseSelect('Sort projects', 'Most recently used');
    expect(shownOrder()).toEqual(['jbs', 'cloud', 'zone']);

    chooseSelect('Sort projects', 'Date added');
    expect(shownOrder()).toEqual(['cloud', 'jbs', 'zone']);

    chooseSelect('Sort projects', 'Name');
    expect(shownOrder()).toEqual(['cloud', 'jbs', 'zone']);

    chooseSelect('Sort projects', 'Status');
    expect(shownOrder()).toEqual(['cloud', 'zone', 'jbs']);
  });

  it('remembers each viewer’s sort', async () => {
    const first = renderPage();
    await loaded();
    chooseSelect('Sort projects', 'Most recently used');
    expect(localStorage.getItem(sortKey('user-1'))).toBe('recent');
    first.unmount();

    const again = renderPage();
    await loaded();
    expect(shownOrder()).toEqual(['jbs', 'cloud', 'zone']);
    again.unmount();

    userId = 'user-2';
    renderPage();
    await loaded();
    expect(shownOrder()).toEqual(['cloud', 'jbs', 'zone']);
  });

  describe('dragging', () => {
    let measure: ReturnType<typeof spyOn>;

    beforeEach(() => {
      const original = HTMLElement.prototype.getBoundingClientRect;
      measure = spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
        this: HTMLElement
      ) {
        const slot = this.closest('.project-drag-overlay')
          ? document.querySelector<HTMLElement>('.project-slot[data-dragging]')
          : this.closest<HTMLElement>('.project-slot');
        if (!slot?.parentElement) return original.call(this);
        const index = Array.from(slot.parentElement.querySelectorAll('.project-slot')).indexOf(
          slot
        );
        const top = index * (SLOT_HEIGHT + 4);
        return {
          x: 0,
          y: top,
          top,
          bottom: top + SLOT_HEIGHT,
          left: 0,
          right: 300,
          width: 300,
          height: SLOT_HEIGHT,
        } as DOMRect;
      });
    });

    afterEach(() => {
      measure.mockRestore();
    });

    async function ready() {
      await loaded();
      await screen.findByRole('button', { name: 'Move Zone' });
    }

    function grip(name: string): HTMLElement {
      return screen.getByRole('button', { name: `Move ${name}` });
    }

    async function press(target: HTMLElement, code: string) {
      await act(async () => {
        fireEvent.keyDown(target, { code, key: code === 'Space' ? ' ' : code });
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }

    function announced(text: string) {
      return waitFor(() => expect(screen.getByText(text)).toBeInTheDocument());
    }

    it('gives each card a grip that says how to move it by keyboard', async () => {
      renderPage();
      await ready();

      const handle = grip('Zone');
      expect(handle).toHaveAttribute('aria-roledescription', 'drag handle');
      expect(
        document.getElementById(handle.getAttribute('aria-describedby') ?? '')
      ).toHaveTextContent(/press space or enter to pick it up/);
    });

    it('gives a viewer who cannot change the workspace no grips', async () => {
      role = 'viewer';
      renderPage();
      await loaded();
      await waitFor(() => expect(mockGetWorkspaceMembers).toHaveBeenCalled());
      await act(() => new Promise((resolve) => setTimeout(resolve, 0)));

      expect(screen.queryByRole('button', { name: /^Move / })).toBeNull();
    });

    it('moves a project by keyboard, saves the order and switches to custom', async () => {
      renderPage();
      await ready();
      chooseSelect('Sort projects', 'Name');
      const handle = grip('Zone');
      handle.focus();

      await press(handle, 'Space');
      await announced('Picked up Zone, at position 3 of 3.');
      await press(handle, 'ArrowUp');
      await announced('Zone would move to position 2 of 3.');
      await press(handle, 'ArrowUp');
      await announced('Zone would move to position 1 of 3.');
      await press(handle, 'Space');

      await announced('Zone dropped at position 1 of 3.');
      expect(mockReorderProjects).toHaveBeenCalledWith('workspace-1', ['zone', 'cloud', 'jbs']);
      expect(screen.getByRole('combobox', { name: 'Sort projects' })).toHaveTextContent('Custom');
      expect(localStorage.getItem(sortKey('user-1'))).toBe('custom');
      await waitFor(() => expect(shownOrder()).toEqual(['zone', 'cloud', 'jbs']));
    });

    it('leaves the order alone when a move is cancelled', async () => {
      renderPage();
      await ready();
      const handle = grip('Cloud');
      handle.focus();

      await press(handle, 'Space');
      await press(handle, 'ArrowDown');
      await announced('Cloud would move to position 2 of 3.');
      await press(handle, 'Escape');

      await announced('Moving Cloud cancelled. It stays at position 1 of 3.');
      expect(mockReorderProjects).not.toHaveBeenCalled();
      expect(shownOrder()).toEqual(['cloud', 'jbs', 'zone']);
    });

    it('does not open the project when its grip is pressed', async () => {
      renderPage();
      await ready();

      fireEvent.click(grip('JBS'));
      fireEvent.keyDown(grip('JBS'), { key: 'Enter' });

      expect(screen.queryByRole('heading', { name: 'JBS', level: 2 })).toBeNull();
    });

    it('puts the order back and says so when saving it fails', async () => {
      mockReorderProjects.mockImplementationOnce(() =>
        Promise.reject(new Error('Workspace write access required'))
      );
      renderPage();
      await ready();
      const handle = grip('Zone');
      handle.focus();

      await press(handle, 'Space');
      await press(handle, 'ArrowUp');
      await announced('Zone would move to position 2 of 3.');
      await press(handle, 'Space');

      const alert = await screen.findByRole('alert');
      expect(within(alert).getByText('Workspace write access required')).toBeInTheDocument();
      await waitFor(() => expect(shownOrder()).toEqual(['cloud', 'jbs', 'zone']));
    });
  });
});
