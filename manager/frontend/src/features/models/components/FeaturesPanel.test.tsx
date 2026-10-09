import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { SetupPlan } from '../../../api/models';

const mockGetSetup = mock();
const mockStartSetup = mock();

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
        id: '8gb',
        label: '8 GB RAM (chat only)',
        min_ram_bytes: 0,
        fast: 'llama3.2:3b',
        reason: 'deepseek-r1:7b',
      },
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
      {
        id: 'pictures',
        label: 'Pictures',
        description: 'Generate stills with FLUX.1 Schnell',
        required: false,
        selected: true,
        blocked: false,
        block_reason: null,
        size_bytes: 17_900_000_000,
        size_label: '18 GB',
        present_bytes: 0,
        needed_bytes: 17_900_000_000,
        ready: false,
        licenses: ['Apache-2.0 (FLUX.1 Schnell)'],
      },
    ],
    wants_all: true,
    gate: null,
    totals: {
      size_bytes: 48_200_000_000,
      size_label: '48 GB',
      present_bytes: 0,
      present_label: '0 B',
      needed_bytes: 48_200_000_000,
      needed_label: '48 GB',
      working_space_bytes: 10_000_000_000,
      working_space_label: '10 GB',
      required_free_bytes: 58_200_000_000,
      required_free_label: '58 GB',
      free_now_bytes: 500_000_000_000,
      free_now_label: '500 GB',
      short_by_bytes: 0,
      short_by_label: null,
    },
    licenses: ['Apache-2.0 (FLUX.1 Schnell)'],
    artifacts: [],
    pulls: [
      { model: 'llama3.1:8b', runtime: 'ollama' },
      { model: 'flux1-schnell-fp8', runtime: 'comfy' },
    ],
    ...overrides,
  };
}

const pull = {
  jobs: [],
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
  pull: mock(() => Promise.resolve(true)),
  reset: mock(),
  cancel: mock(),
  dismiss: mock(),
};

let FeaturesPanel: typeof import('./FeaturesPanel').default;

beforeAll(async () => {
  FeaturesPanel = (await import('./FeaturesPanel')).default;
});

afterAll(() => {
  mock.restore();
});

describe('FeaturesPanel', () => {
  beforeEach(() => {
    mockGetSetup.mockReset();
    mockStartSetup.mockReset();
    pull.pull.mockReset();
    pull.pull.mockImplementation(() => Promise.resolve(true));
    mockGetSetup.mockResolvedValue(plan());
  });

  it('lists features with sizes and disk totals', async () => {
    render(<FeaturesPanel pull={pull} onInstalled={mock()} />);
    expect(await screen.findByRole('heading', { name: 'Features' })).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'Chat' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'Chat' })).toBeDisabled();
    expect(screen.getByText('26 GB')).toBeInTheDocument();
    expect(screen.getByText('Free required')).toBeInTheDocument();
    expect(screen.getByText('58 GB')).toBeInTheDocument();
    expect(screen.getByText('Free now')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install selected' })).toBeEnabled();
  });

  it('marks installed features and remaining download on each row', async () => {
    mockGetSetup.mockResolvedValue(
      plan({
        features: [
          {
            ...plan().features[0],
            present_bytes: 25_600_000_000,
            needed_bytes: 0,
            ready: true,
          },
          {
            ...plan().features[1],
            present_bytes: 1_000_000_000,
            needed_bytes: 3_700_000_000,
            ready: false,
          },
          plan().features[2],
        ],
      })
    );
    render(<FeaturesPanel pull={pull} onInstalled={mock()} />);
    expect(await screen.findByText('Installed')).toBeInTheDocument();
    expect(screen.getByLabelText('Chat: Installed')).toBeInTheDocument();
    expect(screen.getByLabelText('Vision: 3.7 GB left')).toBeInTheDocument();
    expect(screen.getByLabelText('Pictures: 18 GB')).toBeInTheDocument();
    expect(screen.queryByLabelText('Pictures: Installed')).not.toBeInTheDocument();
  });

  it('keeps Installed after unchecking an installed feature', async () => {
    const installed = plan({
      features: [
        {
          ...plan().features[0],
          present_bytes: 25_600_000_000,
          needed_bytes: 0,
          ready: true,
        },
        {
          ...plan().features[1],
          present_bytes: 4_700_000_000,
          needed_bytes: 0,
          ready: true,
        },
        plan().features[2],
      ],
    });
    mockGetSetup.mockImplementation(async (query?: { features?: string[] }) => {
      const selected = query?.features;
      return {
        ...installed,
        features: installed.features.map((feature) => {
          const on = !selected || selected.includes(feature.id);
          if (on) return { ...feature, selected: true };
          return {
            ...feature,
            selected: false,
            present_bytes: 0,
            needed_bytes: 0,
            ready: false,
          };
        }),
      };
    });
    render(<FeaturesPanel pull={pull} onInstalled={mock()} />);
    expect(await screen.findByLabelText('Vision: Installed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Vision' }));
    await waitFor(() => {
      expect(screen.getByRole('checkbox', { name: 'Vision' })).not.toBeChecked();
    });
    expect(screen.getByLabelText('Vision: Installed')).toBeInTheDocument();
    expect(screen.getByLabelText('Chat: Installed')).toBeInTheDocument();
  });

  it('omits the features heading on the first-run setup variant', async () => {
    render(<FeaturesPanel variant="setup" pull={pull} onInstalled={mock()} />);
    expect(await screen.findByRole('checkbox', { name: 'Chat' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Features' })).not.toBeInTheDocument();
    expect(screen.getByText('64 GB RAM · 500 GB free')).toBeInTheDocument();
  });

  it('blocks install when all cannot fit', async () => {
    mockGetSetup.mockResolvedValue(
      plan({
        gate: {
          code: 'all-disk',
          message: 'Cannot select all features: not enough disk.',
        },
        totals: {
          ...plan().totals,
          short_by_bytes: 80_000_000_000,
          short_by_label: '80 GB',
        },
      })
    );
    render(<FeaturesPanel pull={pull} onInstalled={mock()} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Cannot select all features');
    expect(screen.getByText('Short by')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install selected' })).toBeDisabled();
  });

  it('disables vision when RAM is too low', async () => {
    mockGetSetup.mockResolvedValue(
      plan({
        ram_label: '8 GB',
        features: plan().features.map((feature) =>
          feature.id === 'vision'
            ? {
                ...feature,
                selected: false,
                blocked: true,
                block_reason: 'Needs 16 GB RAM (this machine has 8 GB).',
              }
            : { ...feature, selected: feature.id === 'chat' }
        ),
        wants_all: false,
        gate: null,
        pulls: [{ model: 'llama3.2:3b', runtime: 'ollama' }],
      })
    );
    render(<FeaturesPanel pull={pull} onInstalled={mock()} />);
    const vision = await screen.findByRole('checkbox', { name: 'Vision' });
    expect(vision).toBeDisabled();
    expect(vision).not.toBeChecked();
    expect(screen.getByText(/Needs 16 GB RAM/)).toBeInTheDocument();
  });

  it('notifies when the selected features are already on disk', async () => {
    const onInstalled = mock();
    mockGetSetup.mockResolvedValue(plan({ pulls: [] }));
    render(<FeaturesPanel pull={pull} onInstalled={onInstalled} />);
    await screen.findByRole('button', { name: 'Up to date' });
    expect(onInstalled).toHaveBeenCalled();
  });

  it('starts ollama and comfy pulls after a successful plan', async () => {
    const started = plan();
    mockStartSetup.mockResolvedValue(started);
    render(<FeaturesPanel pull={pull} onInstalled={mock()} />);
    await screen.findByRole('button', { name: 'Install selected' });
    fireEvent.click(screen.getByRole('button', { name: 'Install selected' }));
    await waitFor(() => expect(mockStartSetup).toHaveBeenCalled());
    expect(mockStartSetup.mock.calls[0][0]).toEqual({
      features: ['chat', 'vision', 'pictures'],
      chatPreset: '32gb',
    });
    await waitFor(() => expect(pull.pull).toHaveBeenCalledTimes(2));
    expect(pull.pull).toHaveBeenCalledWith('llama3.1:8b', { runtime: undefined });
    expect(pull.pull).toHaveBeenCalledWith('flux1-schnell-fp8', { runtime: 'comfy' });
  });
});
