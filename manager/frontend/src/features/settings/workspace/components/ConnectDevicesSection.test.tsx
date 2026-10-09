import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

const mockClient = {
  getConnectUrls: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

let ConnectDevicesSection: typeof import('./ConnectDevicesSection').default;

beforeAll(async () => {
  ConnectDevicesSection = (await import('./ConnectDevicesSection')).default;
});

afterAll(() => {
  mock.restore();
});

const writeText = mock(() => Promise.resolve());

describe('ConnectDevicesSection', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    writeText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    mockClient.getConnectUrls.mockResolvedValue({
      urls: ['http://192.168.1.10', 'http://100.64.1.2'],
    });
  });

  it('shows a loading state until the URLs arrive', () => {
    mockClient.getConnectUrls.mockImplementation(() => new Promise(() => {}));
    render(<ConnectDevicesSection />);
    expect(screen.getByText('Loading connect URLs...')).toBeInTheDocument();
  });

  it('lists each advertised URL with a copy button', async () => {
    render(<ConnectDevicesSection />);
    expect(await screen.findByText('http://192.168.1.10')).toBeInTheDocument();
    expect(screen.getByText('http://100.64.1.2')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Copy http://192.168.1.10' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Connect a device' })).toBeInTheDocument();
  });

  it('copies a URL to the clipboard', async () => {
    render(<ConnectDevicesSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Copy http://192.168.1.10' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('http://192.168.1.10'));
    expect(
      await screen.findByRole('button', { name: 'Copied http://192.168.1.10' })
    ).toBeInTheDocument();
  });

  it('explains ZONE_CONNECT_URL when nothing is advertised', async () => {
    mockClient.getConnectUrls.mockResolvedValue({ urls: [] });
    render(<ConnectDevicesSection />);
    expect(await screen.findByText(/Set ZONE_CONNECT_URL in \.env/)).toBeInTheDocument();
    expect(screen.getByText(/http:\/\/192\.168\.0\.10/)).toBeInTheDocument();
  });

  it('shows an error when the request fails', async () => {
    mockClient.getConnectUrls.mockRejectedValue(new Error('Failed to fetch connect URLs: 401'));
    render(<ConnectDevicesSection />);
    expect(await screen.findByText('Failed to fetch connect URLs: 401')).toBeInTheDocument();
  });

  it('keeps the heading after a load error', async () => {
    mockClient.getConnectUrls.mockRejectedValue(new Error('offline'));
    render(<ConnectDevicesSection />);
    await waitFor(() => {
      expect(screen.getByText('offline')).toBeInTheDocument();
    });
    expect(screen.getByRole('heading', { name: 'Connect a device' })).toBeInTheDocument();
  });
});
