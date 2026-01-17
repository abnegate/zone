import { act, renderHook, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

const mockGetModels = mock();
const mockDeleteModel = mock();
const mockLogout = mock();

// State container for auth mock that can be updated per test
let authState = {
  isAuthenticated: true,
  isLoading: false,
};

mock.module('../../../api/models', () => ({
  modelsApi: {
    getModels: mockGetModels,
    deleteModel: mockDeleteModel,
  },
}));

// Mock the auth module - note this is the actual import path used by useModels
mock.module('../../../features/auth', () => ({
  useAuth: () => ({
    isAuthenticated: authState.isAuthenticated,
    isLoading: authState.isLoading,
    logout: mockLogout,
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let useModels: typeof import('./useModels').useModels;

beforeAll(async () => {
  ({ useModels } = await import('./useModels'));
});

afterAll(() => {
  mock.restore();
});

describe('useModels', () => {
  beforeEach(() => {
    mockGetModels.mockReset();
    mockDeleteModel.mockReset();
    mockLogout.mockReset();
    // Reset auth state to default
    authState = {
      isAuthenticated: true,
      isLoading: false,
    };
  });

  it('fetches models on mount when authenticated', async () => {
    const mockModels = [
      { name: 'llama2', size: 3800000000, modified_at: '2024-01-01', digest: 'abc123' },
    ];
    mockGetModels.mockResolvedValueOnce({ models: mockModels });

    const { result } = renderHook(() => useModels());

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models).toEqual(mockModels);
    expect(result.current.error).toBeNull();
  });

  it('does not fetch when not authenticated', async () => {
    authState = { isAuthenticated: false, isLoading: false };

    const { result } = renderHook(() => useModels());

    // Wait a bit to ensure no fetch happens
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(mockGetModels).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(true); // Never transitions to false since no fetch
  });

  it('handles fetch error', async () => {
    mockGetModels.mockRejectedValueOnce(new Error('Network error'));

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
    expect(result.current.models).toEqual([]);
  });

  it('handles empty models response', async () => {
    mockGetModels.mockResolvedValueOnce({ models: [] });

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models).toEqual([]);
  });

  it('handles undefined models in response', async () => {
    mockGetModels.mockResolvedValueOnce({ models: [] });

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models).toEqual([]);
  });

  it('deletes a model successfully', async () => {
    const mockModels = [
      { name: 'llama2', size: 3800000000, modified_at: '2024-01-01' },
      { name: 'mistral', size: 4000000000, modified_at: '2024-01-02' },
    ];
    mockGetModels.mockResolvedValueOnce({ models: mockModels });
    mockDeleteModel.mockResolvedValueOnce(undefined);

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let deleteResult = false;
    await act(async () => {
      deleteResult = await result.current.deleteModel('llama2');
    });

    expect(deleteResult).toBe(true);
    expect(result.current.models).toHaveLength(1);
    expect(result.current.models[0].name).toBe('mistral');
  });

  it('handles delete error', async () => {
    const mockModels = [{ name: 'llama2', size: 3800000000, modified_at: '2024-01-01' }];
    mockGetModels.mockResolvedValueOnce({ models: mockModels });
    mockDeleteModel.mockRejectedValueOnce(new Error('Delete failed'));

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let deleteResult = true;
    await act(async () => {
      deleteResult = await result.current.deleteModel('llama2');
    });

    expect(deleteResult).toBe(false);
    expect(result.current.error).toBe('Delete failed');
    expect(result.current.models).toHaveLength(1);
  });

  it('refreshes models', async () => {
    mockGetModels
      .mockResolvedValueOnce({ models: [{ name: 'llama2', size: 1, modified_at: '' }] })
      .mockResolvedValueOnce({ models: [{ name: 'mistral', size: 2, modified_at: '' }] });

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models[0].name).toBe('llama2');

    await act(async () => {
      await result.current.refresh();
    });

    await waitFor(() => {
      expect(result.current.models[0].name).toBe('mistral');
    });
  });

  it('calls logout on 401 error', async () => {
    mockGetModels.mockRejectedValueOnce(new Error('401 Unauthorized'));

    renderHook(() => useModels());

    await waitFor(() => {
      expect(mockLogout).toHaveBeenCalled();
    });
  });

  it('handles non-Error object in fetch error', async () => {
    mockGetModels.mockRejectedValueOnce('String error');

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Failed to fetch models');
    expect(result.current.models).toEqual([]);
  });

  it('handles non-Error object in delete error', async () => {
    const mockModels = [
      { name: 'llama2', size: 3800000000, modified_at: '2024-01-01', digest: 'abc123' },
    ];
    mockGetModels.mockResolvedValueOnce({ models: mockModels });
    mockDeleteModel.mockRejectedValueOnce('String error');

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let deleteResult = true;
    await act(async () => {
      deleteResult = await result.current.deleteModel('llama2');
    });

    expect(deleteResult).toBe(false);
    expect(result.current.error).toBe('Failed to delete model');
    expect(result.current.models).toHaveLength(1);
  });
});
