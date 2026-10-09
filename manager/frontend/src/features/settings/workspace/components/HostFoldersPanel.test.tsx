import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

const mockClient = {
  getHostMounts: mock(),
  getWorkspaceHostDirectories: mock(),
  updateWorkspaceHostDirectories: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

let HostFoldersPanel: typeof import('./HostFoldersPanel').default;

beforeAll(async () => {
  HostFoldersPanel = (await import('./HostFoldersPanel')).default;
});

afterAll(() => {
  mock.restore();
});

describe('HostFoldersPanel', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    mockClient.getHostMounts.mockResolvedValue({
      in_container: true,
      host_root: '/Users/jake/Local',
      container_root: '/host',
      ready: true,
      hint: 'Folders must live under /Users/jake/Local.',
    });
    mockClient.getWorkspaceHostDirectories.mockResolvedValue({
      directories: [],
      folders: [],
    });
    mockClient.updateWorkspaceHostDirectories.mockResolvedValue({
      directories: ['/Users/jake/Local/jbs'],
      folders: [{ host: '/Users/jake/Local/jbs', mapped: '/host/jbs', exists: true }],
    });
  });

  it('saves a host folder for the workspace', async () => {
    const onSaved = mock();
    render(<HostFoldersPanel workspaceId="ws-1" orgId="org-1" variant="setup" onSaved={onSaved} />);
    expect(
      await screen.findByText('Folders must live under /Users/jake/Local.')
    ).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Folder 1'), {
      target: { value: '/Users/jake/Local/jbs' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save and continue' }));
    await waitFor(() =>
      expect(mockClient.updateWorkspaceHostDirectories).toHaveBeenCalledWith('org-1', 'ws-1', {
        directories: ['/Users/jake/Local/jbs'],
      })
    );
    expect(onSaved).toHaveBeenCalled();
    expect(await screen.findByText(/Visible as/)).toBeInTheDocument();
    expect(screen.getByText('/host/jbs')).toBeInTheDocument();
  });

  it('skips when asked', async () => {
    const onSkip = mock();
    render(<HostFoldersPanel workspaceId="ws-1" orgId="org-1" variant="setup" onSkip={onSkip} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Skip for now' }));
    expect(onSkip).toHaveBeenCalled();
  });
});
