import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { AuthenticatedAudio } from './AuthenticatedAudio';

const originalFetch = globalThis.fetch;
const originalCreateObjectUrl = URL.createObjectURL;

const fetchMock = mock();
const createObjectUrlMock = mock(() => 'blob:protected-audio');

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

describe('AuthenticatedAudio', () => {
  it('plays a signed URL so the browser can range-request the media itself', async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        url: '/api/artifacts/chat/track.flac?expires=2000&signature=abc',
        expires_at: 2000,
      }),
    } as Response);

    render(
      <AuthenticatedAudio
        src="/api/artifacts/chat/track.flac"
        label="generated-audio-1.flac"
        accessToken="secret-token"
      />
    );

    expect(screen.getByRole('status')).toHaveTextContent('Loading audio');

    const audio = await screen.findByLabelText('generated-audio-1.flac');
    expect(audio.tagName).toBe('AUDIO');
    expect(audio).toHaveAttribute(
      'src',
      '/api/artifacts/chat/track.flac?expires=2000&signature=abc'
    );
    expect(audio).toHaveAttribute('controls');
    expect(fetchMock).toHaveBeenCalledWith('/api/artifacts/chat/track.flac/signature', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });

  it('renders data and HTTP audio directly without signing it', () => {
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
  });

  it('shows an error when protected audio cannot be signed', async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 403,
      json: async () => ({}),
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
  });
});
