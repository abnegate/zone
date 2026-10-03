import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';

const mockTrainJob = mock(() => Promise.resolve(null as unknown));

let authState = {
  isAuthenticated: true,
};

mock.module('../../../api/models', () => ({
  modelsApi: {
    trainJob: mockTrainJob,
  },
}));

mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: authState.isAuthenticated,
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let useTrainState: typeof import('./useTrain').useTrainState;

beforeAll(async () => {
  const loaded = await import('./useTrain');
  useTrainState = loaded.useTrainState;
});

afterAll(() => {
  mock.restore();
});

describe('useTrain', () => {
  beforeEach(() => {
    mockTrainJob.mockReset();
    mockTrainJob.mockImplementation(() => Promise.resolve(null));
    authState = { isAuthenticated: true };
  });

  it('starts with no job', async () => {
    const { result } = renderHook(() => useTrainState());
    await waitFor(() => expect(result.current.job).toBeNull());
  });

  it('surfaces a running job from the server', async () => {
    mockTrainJob.mockImplementation(() =>
      Promise.resolve({
        id: 'job-1',
        name: 'jerry',
        status: 'running',
        step: 12,
        total: 400,
        eta_seconds: 90,
      })
    );
    const { result } = renderHook(() => useTrainState());
    await waitFor(() => expect(result.current.job?.name).toBe('jerry'));
    expect(result.current.job?.step).toBe(12);
    expect(result.current.job?.eta_seconds).toBe(90);
  });

  it('hides a dismissed job until a new run starts', async () => {
    mockTrainJob.mockImplementation(() =>
      Promise.resolve({
        id: 'job-1',
        name: 'jerry',
        status: 'failed',
        error: 'training loss became NaN',
      })
    );
    const { result } = renderHook(() => useTrainState());
    await waitFor(() => expect(result.current.job?.status).toBe('failed'));
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.job).toBeNull();
  });
});
