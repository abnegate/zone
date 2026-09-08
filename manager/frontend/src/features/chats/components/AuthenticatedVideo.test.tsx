import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { AuthenticatedVideo } from './AuthenticatedVideo';

const originalFetch = globalThis.fetch;
const originalCreateObjectUrl = URL.createObjectURL;

const fetchMock = mock();
const createObjectUrlMock = mock(() => 'blob:protected-video');

beforeEach(() => {
  fetchMock.mockReset();
  createObjectUrlMock.mockClear();
  globalThis.fetch = fetchMock;
  URL.createObjectURL = createObjectUrlMock;
});

afterEach(() => {
  globalThis.fetch = originalFetch;
  URL.createObjectURL = originalCreateObjectUrl;
});

describe('AuthenticatedVideo', () => {
  it('plays a signed URL so the browser can range-request the media itself', async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        url: '/api/artifacts/chat/clip.webm?expires=2000&signature=abc',
        expires_at: 2000,
      }),
    } as Response);

    render(
      <AuthenticatedVideo
        src="/api/artifacts/chat/clip.webm"
        label="generated-video-1.webm"
        accessToken="secret-token"
      />
    );

    expect(screen.getByRole('status')).toHaveTextContent('Loading video');

    const video = await screen.findByLabelText('generated-video-1.webm');
    expect(video.tagName).toBe('VIDEO');
    expect(video).toHaveAttribute(
      'src',
      '/api/artifacts/chat/clip.webm?expires=2000&signature=abc'
    );
    expect(video).toHaveAttribute('controls');
    expect(fetchMock).toHaveBeenCalledWith('/api/artifacts/chat/clip.webm/signature', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });

  it('renders data and HTTP videos directly without signing them', () => {
    const { rerender } = render(
      <AuthenticatedVideo src="data:video/webm;base64,abc" label="Inline video" />
    );

    expect(screen.getByLabelText('Inline video')).toHaveAttribute(
      'src',
      'data:video/webm;base64,abc'
    );

    rerender(
      <AuthenticatedVideo src="https://videos.example.test/clip.webm" label="Remote video" />
    );

    expect(screen.getByLabelText('Remote video')).toHaveAttribute(
      'src',
      'https://videos.example.test/clip.webm'
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('shows an error when a protected video cannot be signed', async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 403,
      json: async () => ({}),
    } as Response);

    render(
      <AuthenticatedVideo
        src="/api/artifacts/chat/denied.webm"
        label="Denied video"
        accessToken="secret-token"
      />
    );

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Video unavailable');
    });
    expect(screen.queryByLabelText('Denied video')).toBeNull();
  });
});
