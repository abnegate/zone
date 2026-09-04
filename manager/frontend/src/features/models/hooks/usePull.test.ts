import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook } from '@testing-library/react';

const mockCreatePullWebSocket = mock();

// State container for auth mock that can be updated per test
let authState = {
  isAuthenticated: true,
  accessToken: 'access-token',
};

mock.module('../../../api/models', () => ({
  modelsApi: {
    createPullWebSocket: mockCreatePullWebSocket,
  },
}));

mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: authState.isAuthenticated,
    accessToken: authState.accessToken,
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let usePull: typeof import('./usePull').usePull;

beforeAll(async () => {
  ({ usePull } = await import('./usePull'));
});

afterAll(() => {
  mock.restore();
});

interface MockWebSocket {
  onopen: (() => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onclose: (() => void) | null;
  onerror: ((error: Event) => void) | null;
  send: ReturnType<typeof mock>;
  close: ReturnType<typeof mock>;
}

function createMockWebSocket(): MockWebSocket {
  return {
    onopen: null,
    onmessage: null,
    onclose: null,
    onerror: null,
    send: mock(),
    close: mock(),
  };
}

describe('usePull', () => {
  beforeEach(() => {
    mockCreatePullWebSocket.mockReset();
    // Reset auth state to default
    authState = {
      isAuthenticated: true,
      accessToken: 'access-token',
    };
  });

  it('initializes with default state', () => {
    const { result } = renderHook(() => usePull());

    expect(result.current.pulling).toBe(false);
    expect(result.current.progress).toBeNull();
    expect(result.current.steps).toEqual([]);
    expect(result.current.result).toBeNull();
  });

  it('does not pull when not authenticated', async () => {
    authState = { isAuthenticated: false, accessToken: '' };

    const { result } = renderHook(() => usePull());

    let pullResult = false;
    await act(async () => {
      pullResult = await result.current.pull('llama2');
    });

    expect(pullResult).toBe(false);
    expect(mockCreatePullWebSocket).not.toHaveBeenCalled();
  });

  it('does not pull with empty model name', async () => {
    const { result } = renderHook(() => usePull());

    let pullResult = false;
    await act(async () => {
      pullResult = await result.current.pull('  ');
    });

    expect(pullResult).toBe(false);
    expect(mockCreatePullWebSocket).not.toHaveBeenCalled();
  });

  it('resets state', () => {
    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.reset();
    });

    expect(result.current.progress).toBeNull();
    expect(result.current.steps).toEqual([]);
    expect(result.current.result).toBeNull();
  });

  it('cancels pull without active WebSocket', () => {
    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.cancel();
    });

    expect(result.current.pulling).toBe(false);
    expect(result.current.result).toEqual({ success: false, message: 'Installation cancelled' });
  });

  it('calls createPullWebSocket when authenticated with valid model name', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    expect(mockCreatePullWebSocket).toHaveBeenCalledWith('llama2');
  });

  it('sets pulling to true when pull starts', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    expect(result.current.pulling).toBe(true);
  });

  it('authenticates before requesting the model', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Trigger onopen
    act(() => {
      mockWs.onopen?.();
    });

    expect(mockWs.send).toHaveBeenCalledWith(
      JSON.stringify({ type: 'auth', token: 'access-token' })
    );
  });

  it('handles authenticated message and sends pull request', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Simulate authenticated message
    act(() => {
      mockWs.onmessage?.({ data: JSON.stringify({ type: 'authenticated' }) } as MessageEvent);
    });

    expect(mockWs.send).toHaveBeenCalledWith(JSON.stringify({ model: 'llama2' }));
  });

  it('handles progress message', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Simulate progress message
    act(() => {
      mockWs.onmessage?.({
        data: JSON.stringify({ type: 'progress', percent: 50 }),
      } as MessageEvent);
    });

    expect(result.current.progress).toBe(50);
  });

  it('handles step message - new step', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Simulate step message
    act(() => {
      mockWs.onmessage?.({
        data: JSON.stringify({
          type: 'step',
          status: 'downloading',
          message: 'Downloading model...',
        }),
      } as MessageEvent);
    });

    expect(result.current.steps).toHaveLength(1);
    expect(result.current.steps[0]).toEqual({
      name: 'downloading',
      message: 'Downloading model...',
      status: 'pending',
    });
  });

  it('handles step message - update existing step', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Add initial step
    act(() => {
      mockWs.onmessage?.({
        data: JSON.stringify({ type: 'step', status: 'downloading', message: 'Starting...' }),
      } as MessageEvent);
    });

    // Update same step
    act(() => {
      mockWs.onmessage?.({
        data: JSON.stringify({ type: 'step', status: 'downloading', message: 'Complete!' }),
      } as MessageEvent);
    });

    expect(result.current.steps).toHaveLength(1);
    expect(result.current.steps[0]).toEqual({
      name: 'downloading',
      message: 'Complete!',
      status: 'success',
    });
  });

  it('handles complete message', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    let _pullResult: boolean | undefined;
    act(() => {
      result.current.pull('llama2').then((r) => {
        _pullResult = r;
      });
    });

    // Simulate complete message
    act(() => {
      mockWs.onmessage?.({
        data: JSON.stringify({ type: 'complete', success: true, message: 'Done!' }),
      } as MessageEvent);
    });

    expect(result.current.pulling).toBe(false);
    expect(result.current.result).toEqual({ success: true, message: 'Done!' });
    expect(mockWs.close).toHaveBeenCalled();
  });

  it('handles complete message with default values', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Simulate complete message without success/message
    act(() => {
      mockWs.onmessage?.({ data: JSON.stringify({ type: 'complete' }) } as MessageEvent);
    });

    expect(result.current.result).toEqual({
      success: true,
      message: 'Model installed successfully',
    });
  });

  it('handles error message', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    let _pullResult: boolean | undefined;
    act(() => {
      result.current.pull('llama2').then((r) => {
        _pullResult = r;
      });
    });

    // Simulate error message
    act(() => {
      mockWs.onmessage?.({
        data: JSON.stringify({ type: 'error', message: 'Download failed' }),
      } as MessageEvent);
    });

    expect(result.current.pulling).toBe(false);
    expect(result.current.result).toEqual({ success: false, message: 'Download failed' });
    expect(mockWs.close).toHaveBeenCalled();
  });

  it('handles error message with default message', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Simulate error message without message
    act(() => {
      mockWs.onmessage?.({ data: JSON.stringify({ type: 'error' }) } as MessageEvent);
    });

    expect(result.current.result).toEqual({ success: false, message: 'Failed to install model' });
  });

  it('handles WebSocket error', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    let _pullResult: boolean | undefined;
    act(() => {
      result.current.pull('llama2').then((r) => {
        _pullResult = r;
      });
    });

    // Simulate WebSocket error
    act(() => {
      mockWs.onerror?.({} as Event);
    });

    expect(result.current.pulling).toBe(false);
    expect(result.current.result).toEqual({ success: false, message: 'Connection error' });
  });

  it('ignores invalid JSON in message', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Simulate invalid JSON message
    act(() => {
      mockWs.onmessage?.({ data: 'invalid json' } as MessageEvent);
    });

    // Should not crash, state unchanged
    expect(result.current.pulling).toBe(true);
    expect(result.current.progress).toBeNull();
  });

  it('cancels pull with active WebSocket', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    expect(result.current.pulling).toBe(true);

    act(() => {
      result.current.cancel();
    });

    expect(mockWs.close).toHaveBeenCalled();
    expect(result.current.pulling).toBe(false);
    expect(result.current.result).toEqual({ success: false, message: 'Installation cancelled' });
  });

  it('trims model name before creating WebSocket', async () => {
    const mockWs = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('  llama2  ');
    });

    expect(mockCreatePullWebSocket).toHaveBeenCalledWith('llama2');
  });
});

describe('pull lifecycle regressions', () => {
  beforeEach(() => {
    mockCreatePullWebSocket.mockReset();
    authState = { isAuthenticated: true, accessToken: 'access-token' };
  });

  for (const outcome of ['error', 'complete', 'close', 'cancel', 'success'] as const) {
    it(`settles pending steps on ${outcome} while preserving completed steps`, async () => {
      const socket = createMockWebSocket();
      mockCreatePullWebSocket.mockReturnValueOnce(socket);
      const { result } = renderHook(() => usePull());
      let pending!: Promise<boolean>;
      act(() => {
        pending = result.current.pull('qwen3.8:27b');
        for (const status of ['pulling manifest', 'pulling manifest', 'downloading']) {
          socket.onmessage?.({ data: JSON.stringify({ type: 'step', status }) } as MessageEvent);
        }
      });
      expect(result.current.steps.map((step) => step.status)).toEqual(['success', 'pending']);

      act(() => {
        if (outcome === 'close') socket.onclose?.();
        else if (outcome === 'cancel') result.current.cancel();
        else {
          socket.onmessage?.({
            data: JSON.stringify({
              type: outcome === 'error' ? 'error' : 'complete',
              success: outcome === 'success',
              message: outcome === 'success' ? 'Done' : 'Download failed',
            }),
          } as MessageEvent);
        }
      });

      expect(await pending).toBe(outcome === 'success');
      expect(result.current.steps.map((step) => step.status)).toEqual([
        'success',
        outcome === 'success' ? 'success' : 'error',
      ]);
      expect(result.current.pulling).toBe(false);
    });
  }

  it('clears failed steps, progress, and result before a successful retry', async () => {
    const first = createMockWebSocket();
    const second = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(first).mockReturnValueOnce(second);
    const { result } = renderHook(() => usePull());
    act(() => {
      result.current.pull('missing');
      first.onmessage?.({
        data: JSON.stringify({ type: 'step', status: 'pulling manifest' }),
      } as MessageEvent);
      first.onmessage?.({
        data: JSON.stringify({ type: 'progress', percent: 50 }),
      } as MessageEvent);
      first.onmessage?.({ data: JSON.stringify({ type: 'error' }) } as MessageEvent);
    });
    expect(result.current.steps[0].status).toBe('error');

    let pending!: Promise<boolean>;
    act(() => {
      pending = result.current.pull('qwen3.8:27b');
    });
    expect(result.current.steps).toEqual([]);
    expect(result.current.progress).toBeNull();
    expect(result.current.result).toBeNull();
    expect(result.current.pulling).toBe(true);
    act(() => {
      second.onmessage?.({
        data: JSON.stringify({ type: 'step', status: 'verifying digest' }),
      } as MessageEvent);
      second.onmessage?.({ data: JSON.stringify({ type: 'complete' }) } as MessageEvent);
    });
    expect(await pending).toBe(true);
    expect(result.current.steps).toEqual([
      { name: 'verifying digest', message: '', status: 'success' },
    ]);
  });

  for (const outcome of ['close', 'cancel', 'unmount'] as const) {
    it(`settles on ${outcome}`, async () => {
      const socket = createMockWebSocket();
      mockCreatePullWebSocket.mockReturnValueOnce(socket);
      const { result, unmount } = renderHook(() => usePull());
      let pending!: Promise<boolean>;
      act(() => {
        pending = result.current.pull('llama2');
      });
      act(() => {
        if (outcome === 'close') socket.onclose?.();
        else if (outcome === 'cancel') result.current.cancel();
        else unmount();
      });
      expect(
        await Promise.race([
          pending,
          new Promise((resolve) => setTimeout(() => resolve('unsettled'), 20)),
        ])
      ).toBe(false);
    });
  }

  it('settles connection construction failure', async () => {
    mockCreatePullWebSocket.mockImplementationOnce(() => {
      throw new Error('blocked');
    });
    const { result } = renderHook(() => usePull());
    await act(async () => {
      expect(await result.current.pull('llama2')).toBe(false);
    });
    expect(result.current.pulling).toBe(false);
  });

  it('sends the model only once and ignores a replaced connection', async () => {
    const first = createMockWebSocket();
    const second = createMockWebSocket();
    mockCreatePullWebSocket.mockReturnValueOnce(first).mockReturnValueOnce(second);
    const { result } = renderHook(() => usePull());
    let pending!: Promise<boolean>;
    act(() => {
      pending = result.current.pull('old');
    });
    const late = first.onmessage;
    act(() => {
      result.current.pull('new');
    });
    expect(await pending).toBe(false);
    act(() => {
      late?.({ data: JSON.stringify({ type: 'error', message: 'Old error' }) } as MessageEvent);
      second.onmessage?.({ data: JSON.stringify({ type: 'authenticated' }) } as MessageEvent);
      second.onmessage?.({ data: JSON.stringify({ type: 'authenticated' }) } as MessageEvent);
    });
    expect(result.current.result).toBeNull();
    expect(second.send).toHaveBeenCalledTimes(1);
  });
});
