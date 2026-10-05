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
    expect(screen.getByRole('complementary', { name: 'Training' })).toBeInTheDocument();
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
    expect(screen.getByRole('complementary', { name: 'Training' })).toBeInTheDocument();
  });

  it('shows a fine-tune phase, weighted percent, loss, and day-scale eta', () => {
    trainState.job = {
      id: 'job-1',
      name: 'jerry',
      status: 'running',
      method: 'finetune',
      step: 0,
      total: 8000,
      phase: 'class_images',
      message: 'Generating class image 12 of 2000',
      percent: 8,
      loss: 0.2134,
      eta_seconds: 7 * 86400,
    };
    render(<TrainDock />);
    expect(screen.getByRole('complementary', { name: 'Training' })).toBeInTheDocument();
    expect(screen.getByText('Fine-tuning jerry')).toBeInTheDocument();
    expect(screen.getByText('Generating class image 12 of 2000')).toBeInTheDocument();
    expect(screen.getAllByText('8%').length).toBeGreaterThan(0);
    expect(screen.getByText(/about 7 days left/)).toBeInTheDocument();
    expect(screen.getByText(/loss 0.2134/)).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '8');
  });

  it('renders snapshot previews from the job', () => {
    trainState.job = {
      id: 'job-1',
      name: 'jerry',
      status: 'running',
      step: 250,
      total: 8000,
      previews: [
        'previews/step-250-0.png',
        'previews/step-250-1.png',
        'previews/step-500-0.png',
        'previews/step-500-1.png',
      ],
    };
    render(<TrainDock />);
    const images = screen.getAllByRole('img');
    expect(images).toHaveLength(4);
    expect(images[0]).toHaveAttribute('src', '/api/models/train/previews/step-250-0.png');
    expect(images[0]).toHaveAttribute('alt', 'Training preview 1 of 4');
  });

  it('names Runpod compute in the headline', () => {
    trainState.job = {
      id: 'job-1',
      name: 'jerry',
      status: 'running',
      method: 'finetune',
      provider: 'runpod',
      gpu: 'A40',
      step: 12,
      total: 400,
    };
    render(<TrainDock />);
    expect(screen.getByText('Fine-tuning jerry on Runpod A40')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute(
      'aria-label',
      'Fine-tuning jerry on Runpod A40'
    );
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
