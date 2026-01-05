import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { client } from '../api/client';
import type { WorkspaceTheme } from '../types';
import WorkspaceSettingsPage from './WorkspaceSettingsPage';

// Mock client
jest.mock('../api/client', () => ({
  client: {
    getWorkspaceTheme: jest.fn(),
    updateWorkspaceTheme: jest.fn(),
    resetWorkspaceTheme: jest.fn(),
  },
}));

// Mock useAuth
jest.mock('../context/AuthContext', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
  }),
}));

// Mock useTheme
const mockSetWorkspaceTheme = jest.fn();
jest.mock('../context/ThemeContext', () => ({
  useTheme: () => ({
    theme: 'light',
    workspaceTheme: null,
    setWorkspaceTheme: mockSetWorkspaceTheme,
  }),
}));

const mockClient = client as jest.Mocked<typeof client>;

const mockTheme: WorkspaceTheme = {
  id: 'theme-1',
  workspace_id: '00000000-0000-0000-0000-000000000001',
  primary_color_light: '#3b82f6',
  secondary_color_light: '#6366f1',
  primary_color_dark: '#60a5fa',
  secondary_color_dark: '#818cf8',
  font_family: 'inter',
  font_size_base: '16px',
  border_radius: 'medium',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-15T00:00:00Z',
};

describe('WorkspaceSettingsPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockClient.getWorkspaceTheme.mockResolvedValue(mockTheme);
  });

  it('shows loading state', async () => {
    mockClient.getWorkspaceTheme.mockImplementation(() => new Promise(() => {}));
    render(<WorkspaceSettingsPage />);
    expect(screen.getByText('Loading theme settings...')).toBeInTheDocument();
  });

  it('shows error when loading fails', async () => {
    mockClient.getWorkspaceTheme.mockRejectedValueOnce(new Error('Failed to load'));
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByText('Failed to load')).toBeInTheDocument();
    });
  });

  it('renders page header', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Workspace Settings' })).toBeInTheDocument();
    });
  });

  it('renders theme configuration section', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Theme Configuration' })).toBeInTheDocument();
    });
  });

  it('renders light mode color settings', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Light Mode Colors' })).toBeInTheDocument();
    });
  });

  it('renders dark mode color settings', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Dark Mode Colors' })).toBeInTheDocument();
    });
  });

  it('renders typography settings', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Typography' })).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Font Family')).toBeInTheDocument();
    expect(screen.getByLabelText('Base Font Size')).toBeInTheDocument();
  });

  it('renders appearance settings', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Appearance' })).toBeInTheDocument();
    });
    expect(screen.getByText('Corner Radius')).toBeInTheDocument();
  });

  it('renders preview section', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Preview' })).toBeInTheDocument();
    });
    expect(screen.getByText(/This is a preview of your theme settings/)).toBeInTheDocument();
  });

  it('renders save and reset buttons', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });
  });

  it('loads and displays current theme values', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(mockClient.getWorkspaceTheme).toHaveBeenCalled();
    });
    expect(mockSetWorkspaceTheme).toHaveBeenCalled();
  });

  it('renders font family options', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByLabelText('Font Family')).toBeInTheDocument();
    });

    const fontSelect = screen.getByLabelText('Font Family');
    expect(fontSelect).toContainHTML('System Default');
    expect(fontSelect).toContainHTML('Inter');
    expect(fontSelect).toContainHTML('Roboto');
  });

  it('renders border radius options', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByText('Corner Radius')).toBeInTheDocument();
    });

    expect(screen.getByLabelText('None')).toBeInTheDocument();
    expect(screen.getByLabelText('Small')).toBeInTheDocument();
    expect(screen.getByLabelText('Medium')).toBeInTheDocument();
    expect(screen.getByLabelText('Large')).toBeInTheDocument();
  });

  it('saves theme changes', async () => {
    const updatedTheme: WorkspaceTheme = {
      ...mockTheme,
      primary_color_light: '#ff0000',
    };
    mockClient.updateWorkspaceTheme.mockResolvedValueOnce(updatedTheme);

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

    await waitFor(() => {
      expect(mockClient.updateWorkspaceTheme).toHaveBeenCalled();
    });
  });

  it('shows success message after save', async () => {
    mockClient.updateWorkspaceTheme.mockResolvedValueOnce(mockTheme);

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

    await waitFor(() => {
      expect(screen.getByText('Theme saved successfully')).toBeInTheDocument();
    });
  });

  it('shows error when save fails', async () => {
    mockClient.updateWorkspaceTheme.mockRejectedValueOnce(new Error('Save failed'));

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

    await waitFor(() => {
      expect(screen.getByText('Save failed')).toBeInTheDocument();
    });
  });

  it('resets theme to defaults', async () => {
    mockClient.resetWorkspaceTheme.mockResolvedValueOnce(mockTheme);

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

    await waitFor(() => {
      expect(mockClient.resetWorkspaceTheme).toHaveBeenCalled();
    });
  });

  it('shows success message after reset', async () => {
    mockClient.resetWorkspaceTheme.mockResolvedValueOnce(mockTheme);

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

    await waitFor(() => {
      expect(screen.getByText('Theme reset to defaults')).toBeInTheDocument();
    });
  });

  it('shows error when reset fails', async () => {
    mockClient.resetWorkspaceTheme.mockRejectedValueOnce(new Error('Reset failed'));

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

    await waitFor(() => {
      expect(screen.getByText('Reset failed')).toBeInTheDocument();
    });
  });

  it('changes font family', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByLabelText('Font Family')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Font Family'), { target: { value: 'roboto' } });

    expect(mockSetWorkspaceTheme).toHaveBeenCalled();
  });

  it('changes font size', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByLabelText('Base Font Size')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Base Font Size'), { target: { value: '18' } });

    expect(screen.getByText('18px')).toBeInTheDocument();
  });

  it('changes border radius', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByLabelText('Large')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByLabelText('Large'));

    expect(mockSetWorkspaceTheme).toHaveBeenCalled();
  });

  it('renders preview buttons', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Primary Button' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Secondary Button' })).toBeInTheDocument();
    });
  });

  it('shows saving state on save button', async () => {
    let resolveUpdate: (value: WorkspaceTheme) => void;
    mockClient.updateWorkspaceTheme.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveUpdate = resolve;
      })
    );

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));

    expect(screen.getByRole('button', { name: 'Saving...' })).toBeDisabled();

    resolveUpdate?.(mockTheme);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeEnabled();
    });
  });

  it('disables reset button while saving', async () => {
    let resolveReset: (value: WorkspaceTheme) => void;
    mockClient.resetWorkspaceTheme.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveReset = resolve;
      })
    );

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

    expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeDisabled();

    resolveReset?.(mockTheme);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeEnabled();
    });
  });
});
