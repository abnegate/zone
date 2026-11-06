import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { clientModule } from '../../../../test/client';
import { chooseSelect, openSelect } from '../../../../test/select';
import type { WorkspaceTheme } from '../types';

const mockClient = {
  updateWorkspaceTheme: mock(),
  resetWorkspaceTheme: mock(),
};

mock.module('../../../../api/client', () => clientModule(mockClient));

mock.module('../../../auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
  }),
}));

const mockSetWorkspaceTheme = mock();
const mockPreviewWorkspaceTheme = mock();
let savedTheme: WorkspaceTheme | null;
let selectedWorkspace: string | null;
let themeLoading = false;
let themeError: string | null = null;

mock.module('../../../../shared/context/ThemeContext', () => ({
  useTheme: () => ({
    theme: 'light',
    workspaceTheme: savedTheme,
    workspaceThemeLoading: themeLoading,
    workspaceThemeError: themeError,
    previewWorkspaceTheme: mockPreviewWorkspaceTheme,
    setWorkspaceTheme: mockSetWorkspaceTheme,
  }),
  workspaceThemeProperties: (value: WorkspaceTheme | null, mode: 'light' | 'dark') =>
    new Map([
      ['--ui-accent', mode === 'dark' ? value?.primary_color_dark : value?.primary_color_light],
      ['font-size', value?.font_size_base],
    ]),
}));

const ORGANIZATION_ID = '00000000-0000-0000-0000-000000000001';
const WORKSPACE_ID = '00000000-0000-0000-0000-000000000001';

mock.module('../../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentOrganization: {
      id: ORGANIZATION_ID,
      name: 'Test Org',
      slug: 'test-org',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    resolvingRole: false,
    currentWorkspace: selectedWorkspace
      ? {
          id: selectedWorkspace,
          organization_id: ORGANIZATION_ID,
          name: 'Test Workspace',
          slug: 'test-workspace',
          description: null,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
        }
      : null,
    organizations: [],
    workspaces: [],
    loading: false,
    error: null,
    setCurrentOrganization: mock(),
    setCurrentWorkspace: mock(),
    refreshOrganizations: mock(),
    refreshWorkspaces: mock(),
  }),
}));

let GeneralSection: typeof import('./GeneralSection').default;

beforeAll(async () => {
  GeneralSection = (await import('./GeneralSection')).default;
});

afterAll(() => {
  mock.restore();
});

const mockTheme: WorkspaceTheme = {
  id: 'theme-1',
  workspace_id: WORKSPACE_ID,
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

const renderSection = (editable = true) => render(<GeneralSection editable={editable} />);

describe('GeneralSection', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    selectedWorkspace = WORKSPACE_ID;
    savedTheme = mockTheme;
    themeLoading = false;
    themeError = null;
    mockClient.updateWorkspaceTheme.mockResolvedValue(mockTheme);
    mockClient.resetWorkspaceTheme.mockResolvedValue(mockTheme);
  });

  it('shows loading state', () => {
    themeLoading = true;
    renderSection();
    expect(screen.getByText('Loading theme settings...')).toBeInTheDocument();
  });

  it('shows error when loading fails', async () => {
    themeError = 'Failed to load settings';
    renderSection();
    await waitFor(() => {
      expect(screen.getByText('Failed to load settings')).toBeInTheDocument();
    });
  });

  it('asks for a workspace when none is selected', () => {
    selectedWorkspace = null;
    renderSection();
    expect(
      screen.getByText('No workspace selected. Please select or create a workspace first.')
    ).toBeInTheDocument();
  });

  it('renders theme configuration section', () => {
    renderSection();
    expect(screen.getByRole('heading', { name: 'Theme Configuration' })).toBeInTheDocument();
  });

  it('renders light and dark mode color settings', () => {
    renderSection();
    expect(screen.getByRole('heading', { name: 'Light Mode Colors' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Dark Mode Colors' })).toBeInTheDocument();
  });

  it('renders typography settings', () => {
    renderSection();
    expect(screen.getByRole('heading', { name: 'Typography & shape' })).toBeInTheDocument();
    expect(screen.getByLabelText('Font Family')).toBeInTheDocument();
    expect(screen.getByLabelText('Base Font Size')).toBeInTheDocument();
    expect(screen.getByText('Corner Radius')).toBeInTheDocument();
  });

  it('renders preview section', () => {
    renderSection();
    expect(screen.getByRole('heading', { name: 'Preview' })).toBeInTheDocument();
    expect(screen.getByText(/This is a preview of your theme settings/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Primary Button' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Secondary Button' })).toBeInTheDocument();
  });

  it('paints the preview with the draft colours, not the app accent', async () => {
    renderSection();
    const preview = await waitFor(() => {
      const box = document.querySelector<HTMLElement>('.preview-box');
      if (!box) throw new Error('no preview box');
      return box;
    });
    expect(preview.style.getPropertyValue('--ui-accent')).toBe('#3b82f6');
    expect(preview.style.fontSize).toBe('16px');

    fireEvent.change(screen.getAllByLabelText('Primary Color hex')[0], {
      target: { value: '#112233' },
    });
    fireEvent.change(screen.getByLabelText('Base Font Size'), { target: { value: '18' } });

    expect(preview.style.getPropertyValue('--ui-accent')).toBe('#112233');
    expect(preview.style.fontSize).toBe('18px');
  });

  it('renders save and reset buttons', () => {
    renderSection();
    expect(screen.getByRole('button', { name: 'Save Changes' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
  });

  it('loads and displays current theme values', async () => {
    renderSection();
    await waitFor(() => {
      expect(screen.getByLabelText('Font Family')).toHaveTextContent('Inter');
    });
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
  });

  it('keeps previews separate from saved settings and clears them on unmount', async () => {
    const { unmount } = renderSection();
    await screen.findByLabelText('Font Family');
    expect(mockPreviewWorkspaceTheme.mock.calls.filter(([value]) => value !== null)).toHaveLength(
      0
    );
    chooseSelect('Font Family', 'Roboto');
    expect(mockPreviewWorkspaceTheme).toHaveBeenLastCalledWith(
      expect.objectContaining({ font_family: 'roboto' })
    );
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
    unmount();
    expect(mockPreviewWorkspaceTheme).toHaveBeenLastCalledWith(null);
  });

  it('saves and resets the theme', async () => {
    mockClient.resetWorkspaceTheme.mockResolvedValue(null);
    renderSection();
    fireEvent.click(await screen.findByRole('button', { name: 'Save Changes' }));
    await screen.findByText('Settings saved successfully');
    expect(mockSetWorkspaceTheme).toHaveBeenLastCalledWith(mockTheme);
    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));
    await screen.findByText('Settings reset to defaults');
    expect(mockClient.resetWorkspaceTheme).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID);
    expect(mockSetWorkspaceTheme).toHaveBeenLastCalledWith(null);
  });

  it('preserves native defaults when editing a partial saved theme', async () => {
    savedTheme = {
      ...mockTheme,
      primary_color_light: null,
      secondary_color_light: null,
      primary_color_dark: null,
      secondary_color_dark: null,
      font_family: null,
      font_size_base: null,
      border_radius: null,
    };
    renderSection();
    await screen.findByLabelText('Font Family');
    chooseSelect('Font Family', 'Roboto');
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    await waitFor(() =>
      expect(mockClient.updateWorkspaceTheme).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID, {
        primary_color_light: null,
        secondary_color_light: null,
        primary_color_dark: null,
        secondary_color_dark: null,
        font_family: 'roboto',
        font_size_base: null,
        border_radius: null,
      })
    );
  });

  it('directly selects explicit system font and medium radius from app defaults', async () => {
    savedTheme = { ...mockTheme, font_family: null, border_radius: null };
    renderSection();
    expect(await screen.findByLabelText('Font Family')).toHaveTextContent('App Default');
    const fontFamily = openSelect('Font Family');
    expect(screen.getByRole('option', { name: 'App Default' })).toHaveAttribute('data-disabled');
    fireEvent.keyDown(fontFamily, { key: 'Escape' });
    expect(screen.getByRole('radio', { name: 'App Default' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'App Default' })).toBeDisabled();
    expect(screen.getByLabelText('Medium')).not.toBeChecked();
    chooseSelect('Font Family', 'System Default');
    fireEvent.click(screen.getByLabelText('Medium'));
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    await waitFor(() =>
      expect(mockClient.updateWorkspaceTheme).toHaveBeenCalledWith(
        ORGANIZATION_ID,
        WORKSPACE_ID,
        expect.objectContaining({ font_family: 'system', border_radius: 'medium' })
      )
    );
  });

  it('resets form values without creating a preview when saved theme becomes null', async () => {
    const { rerender } = renderSection();
    expect(await screen.findByLabelText('Font Family')).toHaveTextContent('Inter');
    savedTheme = null;
    mockPreviewWorkspaceTheme.mockClear();
    rerender(<GeneralSection editable />);
    expect(screen.getByLabelText('Font Family')).toHaveTextContent('Nunito');
    expect(screen.getByLabelText('Large')).toBeChecked();
    expect(screen.queryByRole('radio', { name: 'App Default' })).not.toBeInTheDocument();
    expect(screen.getAllByLabelText('Primary Color hex')[0]).toHaveValue('#0011d9');
    expect(screen.getAllByLabelText('Primary Color hex')[1]).toHaveValue('#00f3ff');
    expect(screen.getAllByLabelText('Secondary Color hex')[0]).toHaveValue('#ecf9ff');
    expect(screen.getAllByLabelText('Secondary Color hex')[1]).toHaveValue('#ecf9ff');
    expect(mockPreviewWorkspaceTheme.mock.calls.filter(([value]) => value !== null)).toHaveLength(
      0
    );
  });

  it('fills the form with product defaults when no theme is saved', async () => {
    savedTheme = null;
    renderSection();
    expect(await screen.findByLabelText('Font Family')).toHaveTextContent('Nunito');
    expect(screen.getByLabelText('Large')).toBeChecked();
    expect(screen.getByText('16px')).toBeInTheDocument();
    expect(screen.getAllByLabelText('Primary Color hex')[0]).toHaveValue('#0011d9');
    expect(screen.getAllByLabelText('Secondary Color hex')[0]).toHaveValue('#ecf9ff');
    expect(screen.getAllByLabelText('Primary Color hex')[1]).toHaveValue('#00f3ff');
    expect(screen.getAllByLabelText('Secondary Color hex')[1]).toHaveValue('#ecf9ff');
    expect(
      document.querySelector<HTMLElement>('.preview-box')?.style.getPropertyValue('--ui-accent')
    ).toBe('#0011d9');
  });

  it('ignores a pending save when switching workspaces', async () => {
    let resolveSave!: (theme: WorkspaceTheme) => void;
    mockClient.updateWorkspaceTheme.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveSave = resolve;
      })
    );
    const { rerender } = renderSection();
    await screen.findByLabelText('Font Family');
    chooseSelect('Font Family', 'Roboto');
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    selectedWorkspace = 'workspace-2';
    savedTheme = null;
    mockPreviewWorkspaceTheme.mockClear();
    rerender(<GeneralSection editable />);
    expect(screen.getByLabelText('Font Family')).toHaveTextContent('Nunito');
    expect(mockPreviewWorkspaceTheme.mock.calls.filter(([value]) => value !== null)).toHaveLength(
      0
    );
    resolveSave(mockTheme);
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Save Changes' })).not.toBeDisabled()
    );
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
    expect(screen.queryByText('Settings saved successfully')).not.toBeInTheDocument();
  });

  it('renders font family options', () => {
    renderSection();
    openSelect('Font Family');
    expect(screen.getByRole('option', { name: 'System Default' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Inter' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Roboto' })).toBeInTheDocument();
  });

  it('renders border radius options', () => {
    renderSection();
    expect(screen.getByLabelText('None')).toBeInTheDocument();
    expect(screen.getByLabelText('Small')).toBeInTheDocument();
    expect(screen.getByLabelText('Medium')).toBeInTheDocument();
    expect(screen.getByLabelText('Large')).toBeInTheDocument();
  });

  it('saves theme changes', async () => {
    mockClient.updateWorkspaceTheme.mockResolvedValueOnce({
      ...mockTheme,
      primary_color_light: '#ff0000',
    });
    renderSection();
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    await waitFor(() => {
      expect(mockClient.updateWorkspaceTheme).toHaveBeenCalled();
    });
  });

  it('shows error when save fails', async () => {
    mockClient.updateWorkspaceTheme.mockRejectedValueOnce(new Error('Save failed'));
    renderSection();
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    expect(await screen.findByText('Save failed')).toBeInTheDocument();
  });

  it('shows error when reset fails', async () => {
    mockClient.resetWorkspaceTheme.mockRejectedValueOnce(new Error('Reset failed'));
    renderSection();
    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));
    expect(await screen.findByText('Reset failed')).toBeInTheDocument();
  });

  it('changes font family without saving', () => {
    renderSection();
    chooseSelect('Font Family', 'Roboto');
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
  });

  it('changes font size', () => {
    renderSection();
    fireEvent.change(screen.getByLabelText('Base Font Size'), { target: { value: '18' } });
    expect(screen.getByText('18px')).toBeInTheDocument();
  });

  it('changes border radius without saving', () => {
    renderSection();
    fireEvent.click(screen.getByLabelText('Large'));
    expect(screen.getByLabelText('Large')).toBeChecked();
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
  });

  it('shows saving state on save button', async () => {
    let resolveUpdate!: (value: WorkspaceTheme) => void;
    mockClient.updateWorkspaceTheme.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveUpdate = resolve;
      })
    );
    renderSection();
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    expect(screen.getByRole('button', { name: 'Saving...' })).toBeDisabled();
    resolveUpdate(mockTheme);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save Changes' })).toBeEnabled();
    });
  });

  it('disables reset button while saving', async () => {
    let resolveReset!: (value: WorkspaceTheme) => void;
    mockClient.resetWorkspaceTheme.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveReset = resolve;
      })
    );
    renderSection();
    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));
    expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeDisabled();
    resolveReset(mockTheme);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeEnabled();
    });
  });

  describe('read-only', () => {
    it('explains why the theme is locked and hides the actions', () => {
      renderSection(false);
      expect(screen.getByText("Viewers can't change the workspace theme.")).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Save Changes' })).toBeNull();
      expect(screen.queryByRole('button', { name: 'Reset to Defaults' })).toBeNull();
    });

    it('disables every theme control but keeps the preview live', () => {
      renderSection(false);
      const lockedFieldset = (element: HTMLElement) => element.closest('fieldset[disabled]');
      expect(screen.getByLabelText('Font Family')).toBeDisabled();
      expect(screen.getByLabelText('Font Family')).toHaveTextContent('Inter');
      for (const control of [
        screen.getByLabelText('Font Family'),
        screen.getByLabelText('Base Font Size'),
        screen.getByLabelText('Medium'),
        ...screen.getAllByLabelText(/Color hex$/),
      ]) {
        expect(lockedFieldset(control)).not.toBeNull();
      }
      const primaryButton = screen.getByRole('button', { name: 'Primary Button' });
      expect(primaryButton).toBeEnabled();
      expect(lockedFieldset(primaryButton)).toBeNull();
    });

    it('never saves or previews from a read-only form', () => {
      const { container } = renderSection(false);
      const form = container.querySelector('form');
      if (!form) throw new Error('no form');
      fireEvent.submit(form);
      expect(mockClient.updateWorkspaceTheme).not.toHaveBeenCalled();
      expect(mockPreviewWorkspaceTheme.mock.calls.filter(([value]) => value !== null)).toHaveLength(
        0
      );
    });

    it('omits the read-only note when editable', () => {
      renderSection();
      expect(screen.queryByText("Viewers can't change the workspace theme.")).toBeNull();
    });
  });
});
