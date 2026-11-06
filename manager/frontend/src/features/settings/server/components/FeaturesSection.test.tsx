import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const mockRefreshModels = mock();
const pull = {
  jobs: [],
  pull: mock(),
  cancel: mock(),
  dismiss: mock(),
  canStart: () => true,
};

type FeaturesPanelProps = {
  pull: typeof pull;
  onInstalled: () => void;
};

mock.module('../../../models', () => ({
  useModels: () => ({
    models: [],
    loading: false,
    error: null,
    refresh: mockRefreshModels,
    deleteModel: mock(),
  }),
  usePull: () => pull,
  FeaturesPanel: ({ pull: received, onInstalled }: FeaturesPanelProps) => (
    <section aria-label="Features">
      <span>{received === pull ? 'Shared pull' : 'Other pull'}</span>
      <button type="button" onClick={onInstalled}>
        Installed
      </button>
    </section>
  ),
}));

const { default: FeaturesSection } = await import('./FeaturesSection');

describe('FeaturesSection', () => {
  beforeEach(() => {
    mockRefreshModels.mockClear();
  });

  it('renders the features picker without a save row', () => {
    render(<FeaturesSection />);
    expect(screen.getByRole('region', { name: 'Features' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save Changes' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reset to Defaults' })).toBeNull();
  });

  it('hands the shared pull queue to the picker', () => {
    render(<FeaturesSection />);
    expect(screen.getByText('Shared pull')).toBeInTheDocument();
  });

  it('refreshes installed models once a feature installs', async () => {
    const user = userEvent.setup();
    render(<FeaturesSection />);
    await user.click(screen.getByRole('button', { name: 'Installed' }));
    expect(mockRefreshModels).toHaveBeenCalledTimes(1);
  });
});
