import { act, renderHook } from '@testing-library/react';
import { client } from '../api/client';
import { usePull } from './usePull';

// Mock the client
jest.mock('../api/client', () => ({
  client: {
    createPullWebSocket: jest.fn(),
    accessToken: 'test-token',
  },
}));

// Mock useAuth hook
jest.mock('../context/AuthContext', () => ({
  useAuth: jest.fn(() => ({
    isAuthenticated: true,
  })),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

import { useAuth } from '../context/AuthContext';

const mockClient = client as jest.Mocked<typeof client>;
const mockUseAuth = useAuth as jest.Mock;

interface MockWebSocket {
  onopen: (() => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onclose: (() => void) | null;
  onerror: ((error: Event) => void) | null;
  send: jest.Mock;
  close: jest.Mock;
}

function createMockWebSocket(): MockWebSocket {
  return {
    onopen: null,
    onmessage: null,
    onclose: null,
    onerror: null,
    send: jest.fn(),
    close: jest.fn(),
  };
}

describe('usePull', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockUseAuth.mockReturnValue({
      isAuthenticated: true,
    });
  });

  it('initializes with default state', () => {
    const { result } = renderHook(() => usePull());

    expect(result.current.pulling).toBe(false);
    expect(result.current.progress).toBeNull();
    expect(result.current.steps).toEqual([]);
    expect(result.current.result).toBeNull();
  });

  it('does not pull when not authenticated', async () => {
    mockUseAuth.mockReturnValue({
      isAuthenticated: false,
    });

    const { result } = renderHook(() => usePull());

    let pullResult = false;
    await act(async () => {
      pullResult = await result.current.pull('llama2');
    });

    expect(pullResult).toBe(false);
    expect(mockClient.createPullWebSocket).not.toHaveBeenCalled();
  });

  it('does not pull with empty model name', async () => {
    const { result } = renderHook(() => usePull());

    let pullResult = false;
    await act(async () => {
      pullResult = await result.current.pull('  ');
    });

    expect(pullResult).toBe(false);
    expect(mockClient.createPullWebSocket).not.toHaveBeenCalled();
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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    expect(mockClient.createPullWebSocket).toHaveBeenCalledWith('llama2');
  });

  it('sets pulling to true when pull starts', async () => {
    const mockWs = createMockWebSocket();
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    expect(result.current.pulling).toBe(true);
  });

  it('sends auth message on WebSocket open', async () => {
    const mockWs = createMockWebSocket();
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('llama2');
    });

    // Trigger onopen
    act(() => {
      mockWs.onopen?.();
    });

    expect(mockWs.send).toHaveBeenCalledWith(JSON.stringify({ token: 'test-token' }));
  });

  it('handles authenticated message and sends pull request', async () => {
    const mockWs = createMockWebSocket();
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

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
    mockClient.createPullWebSocket.mockReturnValueOnce(mockWs as unknown as WebSocket);

    const { result } = renderHook(() => usePull());

    act(() => {
      result.current.pull('  llama2  ');
    });

    expect(mockClient.createPullWebSocket).toHaveBeenCalledWith('llama2');
  });
});
