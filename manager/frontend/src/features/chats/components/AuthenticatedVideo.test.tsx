import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { AuthenticatedVideo } from './AuthenticatedVideo';

const originalFetch = globalThis.fetch;
const originalCreateObjectUrl = URL.createObjectURL;
const originalRevokeObjectUrl = URL.revokeObjectURL;

const fetchMock = mock();
const createObjectUrlMock = mock(() => 'blob:protected-video');
const revokeObjectUrlMock = mock();

beforeEach(() => {
  fetchMock.mockReset();
  createObjectUrlMock.mockClear();
  revokeObjectUrlMock.mockClear();
  globalThis.fetch = fetchMock;
  URL.createObjectURL = createObjectUrlMock;
  URL.revokeObjectURL = revokeObjectUrlMock;
});

afterEach(() => {
  globalThis.fetch = originalFetch;
  URL.createObjectURL = originalCreateObjectUrl;
  URL.revokeObjectURL = originalRevokeObjectUrl;
});

describe('AuthenticatedVideo', () => {
  it('fetches protected artifacts with the bearer token and plays the object URL', async () => {
    const videoBlob = new Blob(['video'], { type: 'video/webm' });
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      blob: async () => videoBlob,
    } as Response);

    const { unmount } = render(
      <AuthenticatedVideo
        src="/api/artifacts/chat/clip.webm"
        label="generated-video-1.webm"
        accessToken="secret-token"
      />
    );

    expect(screen.getByRole('status')).toHaveTextContent('Loading video');

    const video = await screen.findByLabelText('generated-video-1.webm');
    expect(video.tagName).toBe('VIDEO');
    expect(video).toHaveAttribute('src', 'blob:protected-video');
    expect(video).toHaveAttribute('controls');
    expect(fetchMock).toHaveBeenCalledWith('/api/artifacts/chat/clip.webm', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(createObjectUrlMock).toHaveBeenCalledWith(videoBlob);

    unmount();
    expect(revokeObjectUrlMock).toHaveBeenCalledWith('blob:protected-video');
  });

  it('renders data and HTTP videos directly without fetching them', () => {
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
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });

  it('shows an error when a protected video cannot be loaded', async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 403,
      blob: async () => new Blob(),
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
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });
});
