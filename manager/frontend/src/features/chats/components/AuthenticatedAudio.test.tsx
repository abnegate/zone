import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { AuthenticatedAudio } from './AuthenticatedAudio';

const originalFetch = globalThis.fetch;
const originalCreateObjectUrl = URL.createObjectURL;
const originalRevokeObjectUrl = URL.revokeObjectURL;

const fetchMock = mock();
const createObjectUrlMock = mock(() => 'blob:protected-audio');
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

describe('AuthenticatedAudio', () => {
  it('fetches protected artifacts with the bearer token and plays the object URL', async () => {
    const audioBlob = new Blob(['audio'], { type: 'audio/flac' });
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      blob: async () => audioBlob,
    } as Response);

    const { unmount } = render(
      <AuthenticatedAudio
        src="/api/artifacts/chat/track.flac"
        label="generated-audio-1.flac"
        accessToken="secret-token"
      />
    );

    expect(screen.getByRole('status')).toHaveTextContent('Loading audio');

    const audio = await screen.findByLabelText('generated-audio-1.flac');
    expect(audio.tagName).toBe('AUDIO');
    expect(audio).toHaveAttribute('src', 'blob:protected-audio');
    expect(audio).toHaveAttribute('controls');
    expect(fetchMock).toHaveBeenCalledWith('/api/artifacts/chat/track.flac', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(createObjectUrlMock).toHaveBeenCalledWith(audioBlob);

    unmount();
    expect(revokeObjectUrlMock).toHaveBeenCalledWith('blob:protected-audio');
  });

  it('renders data and HTTP audio directly without fetching it', () => {
    const { rerender } = render(
      <AuthenticatedAudio src="data:audio/flac;base64,abc" label="Inline audio" />
    );

    expect(screen.getByLabelText('Inline audio')).toHaveAttribute(
      'src',
      'data:audio/flac;base64,abc'
    );

    rerender(
      <AuthenticatedAudio src="https://audio.example.test/track.flac" label="Remote audio" />
    );

    expect(screen.getByLabelText('Remote audio')).toHaveAttribute(
      'src',
      'https://audio.example.test/track.flac'
    );
    expect(fetchMock).not.toHaveBeenCalled();
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });

  it('shows an error when protected audio cannot be loaded', async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 403,
      blob: async () => new Blob(),
    } as Response);

    render(
      <AuthenticatedAudio
        src="/api/artifacts/chat/denied.flac"
        label="Denied audio"
        accessToken="secret-token"
      />
    );

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Audio unavailable');
    });
    expect(screen.queryByLabelText('Denied audio')).toBeNull();
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });
});
