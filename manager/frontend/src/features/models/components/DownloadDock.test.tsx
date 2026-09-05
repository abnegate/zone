import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';

const pullState = {
  jobs: [] as Array<{
    id: string;
    modelName: string;
    pulling: boolean;
    progress: number | null;
    steps: Array<{ name: string; message: string; status: 'pending' | 'success' | 'error' }>;
    result: { success: boolean; message: string } | null;
  }>,
  activeCount: 0,
  minimized: false,
  setMinimized: mock(),
  cancel: mock(),
  dismiss: mock(),
};

let locationPath = '/chats';

mock.module('../hooks/usePull', () => ({
  usePull: () => pullState,
}));

mock.module('react-router-dom', () => ({
  useLocation: () => ({ pathname: locationPath }),
}));

let DownloadDock: typeof import('./DownloadDock').default;

beforeAll(async () => {
  DownloadDock = (await import('./DownloadDock')).default;
});

afterAll(() => {
  mock.restore();
});

describe('DownloadDock', () => {
  beforeAll(() => {
    pullState.setMinimized.mockReset();
  });

  it('hides on the models screen', () => {
    locationPath = '/models';
    pullState.jobs = [
      {
        id: '1',
        modelName: 'llama2',
        pulling: true,
        progress: 20,
        steps: [],
        result: null,
      },
    ];
    pullState.activeCount = 1;
    pullState.minimized = false;
    const { container } = render(<DownloadDock />);
    expect(container.firstChild).toBeNull();
  });

  it('shows a minimizable popup off the models screen', () => {
    locationPath = '/chats';
    pullState.jobs = [
      {
        id: '1',
        modelName: 'llama2',
        pulling: true,
        progress: 20,
        steps: [],
        result: null,
      },
    ];
    pullState.activeCount = 1;
    pullState.minimized = false;
    render(<DownloadDock />);
    expect(screen.getByRole('complementary', { name: 'Model downloads' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Minimize' }));
    expect(pullState.setMinimized).toHaveBeenCalledWith(true);
  });

  it('expands from the minimized pill', () => {
    locationPath = '/chats';
    pullState.jobs = [
      {
        id: '1',
        modelName: 'llama2',
        pulling: true,
        progress: 20,
        steps: [],
        result: null,
      },
    ];
    pullState.activeCount = 1;
    pullState.minimized = true;
    render(<DownloadDock />);
    fireEvent.click(screen.getByRole('button', { name: /Expand downloads/ }));
    expect(pullState.setMinimized).toHaveBeenCalledWith(false);
  });
});
