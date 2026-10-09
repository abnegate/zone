import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { OrganizationDevice } from '../types';

const mockClient = {
  getDevices: mock(),
  getDevicePolicy: mock(),
  updateDevice: mock(),
  setDevicePolicy: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

let DevicesSection: typeof import('./DevicesSection').default;

beforeAll(async () => {
  DevicesSection = (await import('./DevicesSection')).default;
});

afterAll(() => {
  mock.restore();
});

const phone: OrganizationDevice = {
  id: 'device-1',
  user_id: 'user-1',
  email: 'jake@test.com',
  display_name: 'Jake',
  name: 'S23 Ultra',
  platform: 'android',
  user_agent: 'Zone/1',
  last_ip: '192.168.4.31',
  last_seen_at: '2026-10-09T12:00:00Z',
  status: 'allowed',
  connected: true,
  session_count: 1,
  created_at: '2026-10-09T11:00:00Z',
};

const pending: OrganizationDevice = {
  ...phone,
  id: 'device-2',
  name: 'Chrome',
  platform: 'browser',
  status: 'pending',
  connected: false,
  last_ip: '192.168.4.10',
};

beforeEach(() => {
  mock.clearAllMocks();
  mockClient.getDevices.mockResolvedValue({ devices: [phone, pending] });
  mockClient.getDevicePolicy.mockResolvedValue({ mode: 'open' });
  mockClient.updateDevice.mockImplementation(
    async (_org: string, id: string, request: { status?: string; name?: string }) => ({
      ...(id === phone.id ? phone : pending),
      ...request,
      connected: false,
    })
  );
  mockClient.setDevicePolicy.mockImplementation(async (_org: string, mode: string) => ({ mode }));
});

describe('DevicesSection', () => {
  it('lists connected and pending devices', async () => {
    render(<DevicesSection orgId="org-1" />);
    expect(await screen.findByText('S23 Ultra')).toBeInTheDocument();
    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('pending')).toBeInTheDocument();
    expect(screen.getByText(/192\.168\.4\.31/)).toBeInTheDocument();
  });

  it('allows a pending device', async () => {
    render(<DevicesSection orgId="org-1" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Allow Chrome' }));
    await waitFor(() => {
      expect(mockClient.updateDevice).toHaveBeenCalledWith('org-1', 'device-2', {
        status: 'allowed',
      });
    });
  });

  it('blocks a device after confirm', async () => {
    render(<DevicesSection orgId="org-1" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Block S23 Ultra' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Block' }));
    await waitFor(() => {
      expect(mockClient.updateDevice).toHaveBeenCalledWith('org-1', 'device-1', {
        status: 'blocked',
      });
    });
  });

  it('locks the instance to allowed devices', async () => {
    render(<DevicesSection orgId="org-1" />);
    const lock = await screen.findByLabelText('Only allowed devices may connect');
    fireEvent.click(lock);
    await waitFor(() => {
      expect(mockClient.setDevicePolicy).toHaveBeenCalledWith('org-1', 'allowed');
    });
  });
});
