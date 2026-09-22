import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { PullJob } from '../types';
import PullJobs from './PullJobs';

const pulling: PullJob = {
  id: 'job-1',
  modelName: 'llama3.2:1b',
  pulling: true,
  progress: 42.4,
  chunk: { completed: 500, total: 1000, digest: 'sha256:abcdef123456789' },
  steps: [{ name: 'Downloading', message: 'layer 1 of 3', status: 'pending' }],
  result: null,
};

describe('PullJobs', () => {
  it('lays a job out as one row with its percent and a bar under it', () => {
    const onCancel = mock();
    render(<PullJobs jobs={[pulling]} onCancel={onCancel} onDismiss={mock()} />);

    const row = screen.getByText('Installing model...').closest('.pull-job-row');
    expect(row).not.toBeNull();
    expect(row?.querySelector('.pull-job-name')?.textContent).toBe('llama3.2:1b');
    expect(row?.querySelector('.pull-job-percent')?.textContent).toBe('42%');
    const bar = screen.getByRole('progressbar', { name: 'llama3.2:1b download' });
    expect(bar).toHaveAttribute('aria-valuenow', '42');
    expect(bar.querySelector('.progress-bar-fill')).toHaveStyle({ width: '42.4%' });

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledWith('job-1');
  });

  it('offers Dismiss once a job has finished', () => {
    const onDismiss = mock();
    render(
      <PullJobs
        jobs={[
          {
            ...pulling,
            pulling: false,
            progress: 100,
            result: { success: true, message: 'Model installed!' },
          },
        ]}
        onCancel={mock()}
        onDismiss={onDismiss}
      />
    );

    expect(screen.getByText('Installation complete')).toBeInTheDocument();
    expect(screen.getByText('Model installed!')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalledWith('job-1');
  });
});
