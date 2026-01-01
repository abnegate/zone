import { renderHook, act, waitFor } from '@testing-library/react';
import { useInstallation } from './useInstallation';

const mockFetch = global.fetch as jest.Mock;

describe('useInstallation', () => {
  beforeEach(() => {
    mockFetch.mockClear();
  });

  it('starts with initial state', () => {
    const { result } = renderHook(() => useInstallation());

    expect(result.current.isInstalling).toBe(false);
    expect(result.current.progress).toBe(0);
    expect(result.current.statusLines).toEqual([]);
    expect(result.current.isComplete).toBe(false);
    expect(result.current.error).toBeNull();
    expect(result.current.retryCount).toBe(0);
  });

  it('sets installing state when install called', async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      body: {
        getReader: () => ({
          read: jest.fn().mockResolvedValue({ done: true }),
        }),
      },
    });

    const { result } = renderHook(() => useInstallation());

    act(() => {
      result.current.install({} as any);
    });

    expect(result.current.isInstalling).toBe(true);
  });

  it('handles error response', async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      statusText: 'Internal Server Error',
    });

    const { result } = renderHook(() => useInstallation());

    await act(async () => {
      await result.current.install({} as any);
    });

    await waitFor(() => {
      expect(result.current.error).toContain('Installation failed');
    });
  });

  it('resets state', () => {
    const { result } = renderHook(() => useInstallation());

    act(() => {
      result.current.reset();
    });

    expect(result.current.isInstalling).toBe(false);
    expect(result.current.progress).toBe(0);
    expect(result.current.error).toBeNull();
    expect(result.current.retryCount).toBe(0);
  });

  it('cancels installation', () => {
    const { result } = renderHook(() => useInstallation());

    act(() => {
      result.current.cancel();
    });

    expect(result.current.isInstalling).toBe(false);
    expect(result.current.error).toBe('Installation cancelled');
  });

  it('processes streaming response', async () => {
    const mockReader = {
      read: jest.fn()
        .mockResolvedValueOnce({
          done: false,
          value: new TextEncoder().encode('{"status": "Step 1", "progress": 50}\n'),
        })
        .mockResolvedValueOnce({
          done: false,
          value: new TextEncoder().encode('{"status": "✓ Done", "progress": 100, "complete": true}\n'),
        })
        .mockResolvedValueOnce({ done: true }),
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      body: { getReader: () => mockReader },
    });

    const { result } = renderHook(() => useInstallation());

    await act(async () => {
      await result.current.install({} as any);
    });

    await waitFor(() => {
      expect(result.current.isComplete).toBe(true);
    });
  });
});
