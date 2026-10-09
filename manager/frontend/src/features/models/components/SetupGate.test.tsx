import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import type { SetupPlan } from '../../../api/models';
import { PERMISSIONS } from '../../../shared/types/permissions';
import { setupCompleteKey } from '../setupComplete';

const mockGetSetup = mock();
let authState: {
  user: { id: string } | null;
  hasPermission: (permission: string) => boolean;
};

mock.module('../../../api/models', () => ({
  modelsApi: {
    getSetup: mockGetSetup,
  },
}));

mock.module('../../auth', () => ({
  useAuth: () => authState,
}));

function plan(overrides: Partial<SetupPlan> = {}): SetupPlan {
  return {
    ram_bytes: 1,
    ram_label: '64 GB',
    disk_free_bytes: 1,
    disk_free_label: '500 GB',
    vision_min_ram_bytes: 1,
    disk_margin_bytes: 1,
    chat_preset: '32gb',
    recommended_preset: '32gb',
    chat_presets: [],
    features: [],
    wants_all: true,
    gate: null,
    totals: {
      size_bytes: 0,
      size_label: '0 B',
      present_bytes: 0,
      present_label: '0 B',
      needed_bytes: 0,
      needed_label: '0 B',
      working_space_bytes: 0,
      working_space_label: '0 B',
      required_free_bytes: 0,
      required_free_label: '0 B',
      free_now_bytes: 0,
      free_now_label: '0 B',
      short_by_bytes: 0,
      short_by_label: null,
    },
    licenses: [],
    artifacts: [],
    pulls: [],
    ...overrides,
  };
}

let SetupGate: typeof import('./SetupGate').default;

beforeAll(async () => {
  SetupGate = (await import('./SetupGate')).default;
});

afterAll(() => {
  mock.restore();
});

function renderGate() {
  return render(
    <MemoryRouter initialEntries={['/']}>
      <Routes>
        <Route
          path="/"
          element={
            <SetupGate>
              <div>app</div>
            </SetupGate>
          }
        />
        <Route path="/setup" element={<div>setup</div>} />
      </Routes>
    </MemoryRouter>
  );
}

describe('SetupGate', () => {
  beforeEach(() => {
    localStorage.clear();
    mockGetSetup.mockReset();
    authState = {
      user: { id: 'user-1' },
      hasPermission: (permission: string) => permission === PERMISSIONS.MODELS.READ,
    };
  });

  it('lets the app through when setup was already completed', () => {
    localStorage.setItem(setupCompleteKey('user-1'), '1');
    renderGate();
    expect(screen.getByText('app')).toBeInTheDocument();
    expect(mockGetSetup).not.toHaveBeenCalled();
  });

  it('skips setup when the account cannot read models', () => {
    authState.hasPermission = () => false;
    renderGate();
    expect(screen.getByText('app')).toBeInTheDocument();
    expect(mockGetSetup).not.toHaveBeenCalled();
  });

  it('marks setup complete and stays in the app when nothing remains to download', async () => {
    mockGetSetup.mockResolvedValue(plan({ pulls: [] }));
    renderGate();
    expect(screen.getByText('Loading...')).toBeInTheDocument();
    expect(await screen.findByText('app')).toBeInTheDocument();
    expect(localStorage.getItem(setupCompleteKey('user-1'))).toBe('1');
  });

  it('sends a first-run install to /setup when downloads remain', async () => {
    mockGetSetup.mockResolvedValue(plan({ pulls: [{ model: 'llama3.1:8b', runtime: 'ollama' }] }));
    renderGate();
    expect(await screen.findByText('setup')).toBeInTheDocument();
    expect(screen.queryByText('app')).not.toBeInTheDocument();
  });

  it('opens the app when setup cannot be loaded', async () => {
    mockGetSetup.mockRejectedValue(new Error('offline'));
    renderGate();
    await waitFor(() => expect(screen.getByText('app')).toBeInTheDocument());
  });
});
