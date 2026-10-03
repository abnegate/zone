import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { TrainJob } from '../../../api/models';

const trainState = {
  job: null as TrainJob | null,
  dismiss: mock(),
};

mock.module('../hooks/useTrain', () => ({
  useTrain: () => trainState,
}));

let TrainDock: typeof import('./TrainDock').default;

beforeAll(async () => {
  TrainDock = (await import('./TrainDock')).default;
});

afterAll(() => {
  mock.restore();
});

describe('TrainDock', () => {
  beforeAll(() => {
    trainState.dismiss.mockReset();
  });

  it('hides when there is no job', () => {
    trainState.job = null;
    const { container } = render(<TrainDock />);
    expect(container.firstChild).toBeNull();
  });

  it('shows a progress bar and eta in the nav', () => {
    trainState.job = {
      id: 'job-1',
      name: 'jerry',
      status: 'running',
      step: 40,
      total: 400,
      eta_seconds: 180,
    };
    render(<TrainDock />);
    expect(screen.getByRole('complementary', { name: 'LoRA training' })).toBeInTheDocument();
    expect(screen.getByText('Training jerry')).toBeInTheDocument();
    expect(screen.getByText('Step 40 of 400')).toBeInTheDocument();
    expect(screen.getAllByText('10%').length).toBeGreaterThan(0);
    expect(screen.getByText(/about 3 minutes left/)).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '10');
    expect(screen.queryByRole('button', { name: 'Dismiss' })).not.toBeInTheDocument();
  });

  it('stays visible on the models screen', () => {
    trainState.job = {
      id: 'job-1',
      name: 'jerry',
      status: 'running',
      step: 0,
      total: 400,
    };
    render(<TrainDock />);
    expect(screen.getByText('Starting training')).toBeInTheDocument();
    expect(screen.getByRole('complementary', { name: 'LoRA training' })).toBeInTheDocument();
  });

  it('lets a finished failure be dismissed', () => {
    trainState.job = {
      id: 'job-1',
      name: 'jerry',
      status: 'failed',
      error: 'training loss became NaN',
    };
    render(<TrainDock />);
    expect(screen.getByText('training loss became NaN')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(trainState.dismiss).toHaveBeenCalled();
  });
});
