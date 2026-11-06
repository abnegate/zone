import { afterAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { SetupPlan } from '../../../../api/models';
import { MODEL_ACCESS_NOTICE } from '../../../models/components/ModelAccessNotice';

const mockGetSetup = mock();
const mockStartSetup = mock();

mock.module('../../../../api/models', () => ({
  modelsApi: {
    getSetup: mockGetSetup,
    startSetup: mockStartSetup,
  },
  SetupError: class SetupError extends Error {},
}));

const access = { canInstall: true, canRemove: true };

mock.module('../../../models/hooks/useModelAccess', () => ({
  useModelAccess: () => access,
}));

const pull = {
  jobs: [],
  canStart: () => true,
  pull: mock(() => Promise.resolve(true)),
  cancel: mock(),
  dismiss: mock(),
};

const { default: FeaturesPanel } = await import('../../../models/components/FeaturesPanel');

mock.module('../../../models', () => ({
  FeaturesPanel,
  useModels: () => ({ refresh: mock() }),
  usePull: () => pull,
}));

const { default: FeaturesSection } = await import('./FeaturesSection');

const plan: SetupPlan = {
  ram_bytes: 64 * 1024 ** 3,
  ram_label: '64 GB',
  disk_free_bytes: 500_000_000_000,
  disk_free_label: '500 GB',
  vision_min_ram_bytes: 16 * 1024 ** 3,
  disk_margin_bytes: 10_000_000_000,
  chat_preset: '8gb',
  recommended_preset: '8gb',
  chat_presets: [
    {
      id: '8gb',
      label: '8 GB RAM (chat only)',
      min_ram_bytes: 0,
      fast: 'llama3.2:3b',
      reason: 'deepseek-r1:7b',
    },
  ],
  features: [
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
  wants_all: false,
  gate: null,
  totals: {
    size_bytes: 4_700_000_000,
    size_label: '4.7 GB',
    present_bytes: 0,
    present_label: '0 B',
    needed_bytes: 4_700_000_000,
    needed_label: '4.7 GB',
    working_space_bytes: 10_000_000_000,
    working_space_label: '10 GB',
    required_free_bytes: 14_700_000_000,
    required_free_label: '15 GB',
    free_now_bytes: 500_000_000_000,
    free_now_label: '500 GB',
    short_by_bytes: 0,
    short_by_label: null,
  },
  licenses: [],
  artifacts: [],
  pulls: [{ model: 'llava:7b', runtime: 'ollama' }],
};

afterAll(() => {
  mock.restore();
});

describe('FeaturesSection model access', () => {
  beforeEach(() => {
    mockGetSetup.mockReset();
    mockGetSetup.mockResolvedValue(plan);
    access.canInstall = true;
  });

  it('shows a read-only plan to a model reader who cannot install', async () => {
    access.canInstall = false;
    render(<FeaturesSection />);
    expect(await screen.findByRole('heading', { name: 'Features' })).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'Vision' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Install selected' })).not.toBeInTheDocument();
    expect(screen.getByText(MODEL_ACCESS_NOTICE)).toHaveAttribute('role', 'note');
    expect(mockStartSetup).not.toHaveBeenCalled();
  });

  it('offers the install action to a user who can install', async () => {
    render(<FeaturesSection />);
    expect(await screen.findByRole('button', { name: 'Install selected' })).toBeInTheDocument();
    expect(screen.queryByText(MODEL_ACCESS_NOTICE)).not.toBeInTheDocument();
  });
});
