import { act, renderHook, waitFor } from '@testing-library/react';
import { client } from '../api/client';
import { useModels } from './useModels';

// Mock the client
jest.mock('../api/client', () => ({
  client: {
    getModels: jest.fn(),
    deleteModel: jest.fn(),
  },
}));

// Mock useAuth hook
jest.mock('../context/AuthContext', () => ({
  useAuth: jest.fn(() => ({
    isAuthenticated: true,
    logout: jest.fn(),
  })),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

import { useAuth } from '../context/AuthContext';

const mockClient = client as jest.Mocked<typeof client>;
const mockUseAuth = useAuth as jest.Mock;

describe('useModels', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockUseAuth.mockReturnValue({
      isAuthenticated: true,
      logout: jest.fn(),
    });
  });

  it('fetches models on mount when authenticated', async () => {
    const mockModels = [
      { name: 'llama2', size: 3800000000, modified_at: '2024-01-01', digest: 'abc123' },
    ];
    mockClient.getModels.mockResolvedValueOnce({ models: mockModels });

    const { result } = renderHook(() => useModels());

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models).toEqual(mockModels);
    expect(result.current.error).toBeNull();
  });

  it('does not fetch when not authenticated', async () => {
    mockUseAuth.mockReturnValue({
      isAuthenticated: false,
      logout: jest.fn(),
    });

    const { result } = renderHook(() => useModels());

    // Wait a bit to ensure no fetch happens
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(mockClient.getModels).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(true); // Never transitions to false since no fetch
  });

  it('handles fetch error', async () => {
    mockClient.getModels.mockRejectedValueOnce(new Error('Network error'));

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
    expect(result.current.models).toEqual([]);
  });

  it('handles empty models response', async () => {
    mockClient.getModels.mockResolvedValueOnce({ models: [] });

    const { result } = renderHook(() => useModels());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.models).toEqual([]);
  });

  it('handles undefined models in response', async () => {
    mockClient.getModels.mockResolvedValueOnce({ models: [] });

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
    mockClient.getModels.mockResolvedValueOnce({ models: mockModels });
    mockClient.deleteModel.mockResolvedValueOnce(undefined);

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
    mockClient.getModels.mockResolvedValueOnce({ models: mockModels });
    mockClient.deleteModel.mockRejectedValueOnce(new Error('Delete failed'));

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
    mockClient.getModels
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
    const logoutMock = jest.fn();
    mockUseAuth.mockReturnValue({
      isAuthenticated: true,
      logout: logoutMock,
    });
    mockClient.getModels.mockRejectedValueOnce(new Error('401 Unauthorized'));

    renderHook(() => useModels());

    await waitFor(() => {
      expect(logoutMock).toHaveBeenCalled();
    });
  });

  it('handles non-Error object in fetch error', async () => {
    mockClient.getModels.mockRejectedValueOnce('String error');

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
    mockClient.getModels.mockResolvedValueOnce({ models: mockModels });
    mockClient.deleteModel.mockRejectedValueOnce('String error');

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
