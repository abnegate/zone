import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { client } from '../../../api/client';
import { isProtectedArtifactUrl } from '../api/protectedImages';
import { AuthenticatedImage } from './AuthenticatedImage';

const originalFetch = globalThis.fetch;
const originalCreateObjectUrl = URL.createObjectURL;
const originalRevokeObjectUrl = URL.revokeObjectURL;

const fetchMock = mock();
const createObjectUrlMock = mock(() => 'blob:protected-image');
const revokeObjectUrlMock = mock();

beforeEach(() => {
  fetchMock.mockReset();
  createObjectUrlMock.mockClear();
  revokeObjectUrlMock.mockClear();
  client.setAccessToken('secret-token');
  globalThis.fetch = fetchMock;
  URL.createObjectURL = createObjectUrlMock;
  URL.revokeObjectURL = revokeObjectUrlMock;
});

afterEach(() => {
  client.setAccessToken(null);
  globalThis.fetch = originalFetch;
  URL.createObjectURL = originalCreateObjectUrl;
  URL.revokeObjectURL = originalRevokeObjectUrl;
});

describe('AuthenticatedImage', () => {
  it('fetches protected artifacts with the bearer token and opens the object URL', async () => {
    const imageBlob = new Blob(['image'], { type: 'image/webp' });
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      blob: async () => imageBlob,
    } as Response);

    const { unmount } = render(
      <AuthenticatedImage src="/api/artifacts/chat/image.webp" alt="Generated landscape" />
    );

    expect(screen.getByRole('status')).toHaveTextContent('Loading image');

    const image = await screen.findByRole('img', { name: 'Generated landscape' });
    expect(image).toHaveAttribute('src', 'blob:protected-image');
    expect(
      screen.getByRole('link', { name: 'Open Generated landscape full size' })
    ).toHaveAttribute('href', 'blob:protected-image');
    expect(fetchMock).toHaveBeenCalledWith('/api/artifacts/chat/image.webp', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(createObjectUrlMock).toHaveBeenCalledWith(imageBlob);

    unmount();
    expect(revokeObjectUrlMock).toHaveBeenCalledWith('blob:protected-image');
  });

  it('renders data and HTTP images directly without fetching them', () => {
    const { rerender } = render(
      <AuthenticatedImage src="data:image/png;base64,abc" alt="Inline image" />
    );

    expect(screen.getByRole('img', { name: 'Inline image' })).toHaveAttribute(
      'src',
      'data:image/png;base64,abc'
    );

    rerender(<AuthenticatedImage src="https://images.example.test/photo.png" alt="Remote image" />);

    expect(screen.getByRole('img', { name: 'Remote image' })).toHaveAttribute(
      'src',
      'https://images.example.test/photo.png'
    );
    expect(fetchMock).not.toHaveBeenCalled();
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });

  it('shows an error when a protected image cannot be loaded', async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 403,
      blob: async () => new Blob(),
    } as Response);

    render(<AuthenticatedImage src="/api/artifacts/chat/denied.webp" alt="Denied image" />);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Image unavailable');
    });
    expect(screen.queryByRole('img')).toBeNull();
    expect(createObjectUrlMock).not.toHaveBeenCalled();
  });

  it('never treats absolute URLs as protected artifact URLs', () => {
    expect(isProtectedArtifactUrl('/api/artifacts/chat/image.webp')).toBe(true);
    expect(isProtectedArtifactUrl('https://other.example/api/artifacts/chat/image.webp')).toBe(
      false
    );
  });
});
