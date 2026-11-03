import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { isProtectedArtifactUrl, isProtectedImageUrl } from '../../../api/protectedImages';
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
  globalThis.fetch = fetchMock;
  URL.createObjectURL = createObjectUrlMock;
  URL.revokeObjectURL = revokeObjectUrlMock;
});

afterEach(() => {
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
      <AuthenticatedImage
        src="/api/artifacts/chat/image.webp"
        alt="Generated landscape"
        accessToken="secret-token"
      />
    );

    expect(screen.getByRole('status')).toHaveTextContent('Loading image');

    const image = await screen.findByRole('img', { name: 'Generated landscape' });
    expect(image).toHaveAttribute('src', 'blob:protected-image');
    expect(
      screen.getByRole('link', { name: 'Open Generated landscape full size' })
    ).toHaveAttribute('href', '/api/artifacts/chat/image.webp');
    expect(fetchMock).toHaveBeenCalledWith('/api/artifacts/chat/image.webp', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(createObjectUrlMock).toHaveBeenCalledWith(imageBlob);

    const opened = { closed: false };
    const openMock = mock(() => opened);
    const originalOpen = window.open;
    window.open = openMock as typeof window.open;
    createObjectUrlMock.mockImplementationOnce(() => 'blob:full-size');
    fireEvent.click(screen.getByRole('link', { name: 'Open Generated landscape full size' }));
    await waitFor(() => {
      expect(openMock).toHaveBeenCalledWith('blob:full-size', '_blank', 'noopener,noreferrer');
    });
    window.open = originalOpen;

    unmount();
    expect(revokeObjectUrlMock).toHaveBeenCalledWith('blob:protected-image');
    expect(revokeObjectUrlMock).not.toHaveBeenCalledWith('blob:full-size');
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

    render(
      <AuthenticatedImage
        src="/api/artifacts/chat/denied.webp"
        alt="Denied image"
        accessToken="secret-token"
      />
    );

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

  it('fetches train previews with the bearer token instead of an img src', async () => {
    const imageBlob = new Blob(['preview'], { type: 'image/png' });
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      blob: async () => imageBlob,
    } as Response);

    render(
      <AuthenticatedImage
        src="/api/models/train/previews/step-250-0.png"
        alt="Training preview 1 of 4"
        accessToken="secret-token"
        linked={false}
        compact
        fallbackClassName="train-meter-preview"
      />
    );

    expect(screen.getByRole('status', { name: 'Loading image' })).toHaveClass(
      'train-meter-preview'
    );
    const image = await screen.findByRole('img', { name: 'Training preview 1 of 4' });
    expect(image).toHaveAttribute('src', 'blob:protected-image');
    expect(fetchMock).toHaveBeenCalledWith('/api/models/train/previews/step-250-0.png', {
      headers: { Authorization: 'Bearer secret-token' },
      signal: expect.any(AbortSignal),
    });
    expect(isProtectedImageUrl('/api/models/train/previews/step-250-0.png')).toBe(true);
    expect(isProtectedArtifactUrl('/api/models/train/previews/step-250-0.png')).toBe(false);
    expect(
      isProtectedImageUrl('https://example.test/api/models/train/previews/step-250-0.png')
    ).toBe(false);
  });
});
