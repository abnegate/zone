import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import fixture from '../../../../../../../runner/zone_server/tests/fixtures/agents.json';
import type { OrgRole } from '../../organization/types';
import type { AiSettings, WorkspaceAiSettings, WorkspaceTheme } from '../types';

// Mock client
const mockClient = {
  getWorkspaceTheme: mock(),
  updateWorkspaceTheme: mock(),
  resetWorkspaceTheme: mock(),
  getWorkspaceAiSettings: mock(),
  updateWorkspaceAiSettings: mock(),
  resetWorkspaceAiSettings: mock(),
  getEffectiveAiSettings: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

const agentsApi = {
  list: mock(),
  get: mock(),
  start: mock(),
  submitCode: mock(),
  signOut: mock(),
};

mock.module('../../../../api/agents', () => ({ agentsApi }));

// Mock useAuth
const mockUseAuth = mock(() => ({
  isAuthenticated: true,
  user: { id: '1', email: 'test@test.com' },
}));

mock.module('../../../auth', () => ({
  useAuth: mockUseAuth,
}));

// Mock useTheme
const mockSetWorkspaceTheme = mock();
const mockPreviewWorkspaceTheme = mock();
let savedTheme: WorkspaceTheme | null;
let selectedWorkspace = '00000000-0000-0000-0000-000000000001';
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

mock.module('../../../models', () => ({
  useModels: () => ({
    models: [],
    loading: false,
    error: null,
    refresh: mock(),
    deleteModel: mock(),
  }),
}));

const organization = {
  id: '00000000-0000-0000-0000-000000000001',
  name: 'Test Org',
  slug: 'test-org',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};
const organizations: Record<'unknown' | OrgRole, typeof organization & { role?: OrgRole }> = {
  unknown: organization,
  owner: { ...organization, role: 'owner' },
  admin: { ...organization, role: 'admin' },
  member: { ...organization, role: 'member' },
};
let organizationRole: 'unknown' | OrgRole = 'unknown';
let resolvingRole = false;

mock.module('../../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentOrganization: organizations[organizationRole],
    resolvingRole,
    currentWorkspace: {
      id: selectedWorkspace,
      organization_id: '00000000-0000-0000-0000-000000000001',
      name: 'Test Workspace',
      slug: 'test-workspace',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
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

let WorkspaceSettingsPage: typeof import('./WorkspaceSettingsPage').default;

beforeAll(async () => {
  WorkspaceSettingsPage = (await import('./WorkspaceSettingsPage')).default;
});

afterAll(() => {
  mock.restore();
});

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

const mockAiSettings: AiSettings = {
  provider: 'self_hosted',
  has_litellm_key: false,
  litellm_host: null,
  has_openai_api_key: false,
  openai_base_url: null,
  has_anthropic_api_key: false,
  anthropic_base_url: null,
  bedrock_region: null,
  bedrock_use_iam_role: false,
  has_bedrock_credentials: false,
  model_fast: 'llama3.1:8b',
  model_reasoning: 'deepseek-r1:7b',
  model_embedding: 'nomic-embed-text',
  model_image: 'flux1-schnell-fp8.safetensors',
  model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
  model_audio: 'ace_step_v1_3.5b.safetensors',
};

const savedAiSettings: WorkspaceAiSettings = { ...mockAiSettings, overrides: true };

const inheritedAiSettings: WorkspaceAiSettings = {
  ...mockAiSettings,
  model_fast: null,
  model_reasoning: null,
  model_embedding: null,
  model_image: null,
  model_video: null,
  model_audio: null,
  overrides: false,
};

describe('WorkspaceSettingsPage', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    organizationRole = 'unknown';
    resolvingRole = false;
    agentsApi.list.mockResolvedValue(fixture.agents);
    selectedWorkspace = '00000000-0000-0000-0000-000000000001';
    savedTheme = mockTheme;
    themeLoading = false;
    themeError = null;
    mockClient.getWorkspaceTheme.mockResolvedValue(mockTheme);
    mockClient.getWorkspaceAiSettings.mockResolvedValue(savedAiSettings);
    mockClient.getEffectiveAiSettings.mockResolvedValue(mockAiSettings);
    mockClient.updateWorkspaceTheme.mockResolvedValue(mockTheme);
    mockClient.updateWorkspaceAiSettings.mockResolvedValue(savedAiSettings);
    mockClient.resetWorkspaceTheme.mockResolvedValue(mockTheme);
    mockClient.resetWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);
  });

  it('shows loading state', async () => {
    themeLoading = true;
    render(<WorkspaceSettingsPage />);
    expect(screen.getByText('Loading theme settings...')).toBeInTheDocument();
  });

  it('shows error when loading fails', async () => {
    themeError = 'Failed to load settings';
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByText('Failed to load settings')).toBeInTheDocument();
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
      expect(screen.getByRole('heading', { name: 'Typography & shape' })).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Font Family')).toBeInTheDocument();
    expect(screen.getByLabelText('Base Font Size')).toBeInTheDocument();
  });

  it('renders appearance settings', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {});
    expect(screen.getByText('Corner Radius')).toBeInTheDocument();
  });

  it('renders preview section', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Preview' })).toBeInTheDocument();
    });
    expect(screen.getByText(/This is a preview of your theme settings/)).toBeInTheDocument();
  });

  it('paints the preview with the draft colours, not the app accent', async () => {
    render(<WorkspaceSettingsPage />);
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
      expect(screen.getByLabelText('Font Family')).toHaveValue('inter');
    });
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
  });

  it('keeps previews separate from saved settings and clears them on unmount', async () => {
    const { unmount } = render(<WorkspaceSettingsPage />);
    await screen.findByLabelText('Font Family');
    expect(mockPreviewWorkspaceTheme.mock.calls.filter(([value]) => value !== null)).toHaveLength(
      0
    );
    fireEvent.change(screen.getByLabelText('Font Family'), { target: { value: 'roboto' } });
    expect(mockPreviewWorkspaceTheme).toHaveBeenLastCalledWith(
      expect.objectContaining({ font_family: 'roboto' })
    );
    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
    unmount();
    expect(mockPreviewWorkspaceTheme).toHaveBeenLastCalledWith(null);
  });

  it('saves and resets theme without touching AI settings even when AI fails', async () => {
    mockClient.getWorkspaceAiSettings.mockRejectedValue(new Error('AI unavailable'));
    mockClient.resetWorkspaceTheme.mockResolvedValue(null);
    render(<WorkspaceSettingsPage />);
    fireEvent.click(await screen.findByRole('button', { name: 'Save Changes' }));
    await screen.findByText('Settings saved successfully');
    expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
    expect(mockClient.getWorkspaceAiSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));
    await screen.findByText('Settings reset to defaults');
    expect(mockClient.resetWorkspaceAiSettings).not.toHaveBeenCalled();
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
    render(<WorkspaceSettingsPage />);
    fireEvent.change(await screen.findByLabelText('Font Family'), { target: { value: 'roboto' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    await waitFor(() =>
      expect(mockClient.updateWorkspaceTheme).toHaveBeenCalledWith(
        expect.any(String),
        expect.any(String),
        {
          primary_color_light: null,
          secondary_color_light: null,
          primary_color_dark: null,
          secondary_color_dark: null,
          font_family: 'roboto',
          font_size_base: null,
          border_radius: null,
        }
      )
    );
  });

  it('directly selects explicit system font and medium radius from app defaults', async () => {
    savedTheme = { ...mockTheme, font_family: null, border_radius: null };
    render(<WorkspaceSettingsPage />);
    expect(await screen.findByLabelText('Font Family')).toHaveValue('');
    expect(screen.getByRole('option', { name: 'App Default' })).toBeDisabled();
    expect(screen.getByRole('radio', { name: 'App Default' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'App Default' })).toBeDisabled();
    expect(screen.getByLabelText('Medium')).not.toBeChecked();
    fireEvent.change(screen.getByLabelText('Font Family'), { target: { value: 'system' } });
    fireEvent.click(screen.getByLabelText('Medium'));
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    await waitFor(() =>
      expect(mockClient.updateWorkspaceTheme).toHaveBeenCalledWith(
        expect.any(String),
        expect.any(String),
        expect.objectContaining({ font_family: 'system', border_radius: 'medium' })
      )
    );
  });

  it('resets form values without creating a preview when saved theme becomes null', async () => {
    const { rerender } = render(<WorkspaceSettingsPage />);
    expect(await screen.findByLabelText('Font Family')).toHaveValue('inter');
    savedTheme = null;
    mockPreviewWorkspaceTheme.mockClear();
    rerender(<WorkspaceSettingsPage />);
    expect(screen.getByLabelText('Font Family')).toHaveValue('');
    expect(mockPreviewWorkspaceTheme.mock.calls.filter(([value]) => value !== null)).toHaveLength(
      0
    );
  });

  it('ignores a pending save when switching workspaces', async () => {
    let resolveSave!: (theme: WorkspaceTheme) => void;
    mockClient.updateWorkspaceTheme.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveSave = resolve;
      })
    );
    const { rerender } = render(<WorkspaceSettingsPage />);
    fireEvent.change(await screen.findByLabelText('Font Family'), { target: { value: 'roboto' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save Changes' }));
    selectedWorkspace = 'workspace-2';
    savedTheme = null;
    mockPreviewWorkspaceTheme.mockClear();
    rerender(<WorkspaceSettingsPage />);
    expect(screen.getByLabelText('Font Family')).toHaveValue('');
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
      expect(screen.getByText('Settings saved successfully')).toBeInTheDocument();
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
    mockClient.resetWorkspaceAiSettings.mockResolvedValueOnce(inheritedAiSettings);
    mockClient.getEffectiveAiSettings.mockResolvedValueOnce(mockAiSettings);

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
    mockClient.resetWorkspaceAiSettings.mockResolvedValueOnce(inheritedAiSettings);
    mockClient.getEffectiveAiSettings.mockResolvedValueOnce(mockAiSettings);

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

    await waitFor(() => {
      expect(screen.getByText('Settings reset to defaults')).toBeInTheDocument();
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

    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
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

    expect(mockSetWorkspaceTheme).not.toHaveBeenCalled();
  });

  it('renders preview buttons', async () => {
    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Primary Button' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Secondary Button' })).toBeInTheDocument();
    });
  });

  it('shows saving state on save button', async () => {
    let resolveUpdate!: (value: WorkspaceTheme) => void;
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
    mockClient.resetWorkspaceAiSettings.mockResolvedValueOnce(inheritedAiSettings);
    mockClient.getEffectiveAiSettings.mockResolvedValueOnce(mockAiSettings);

    render(<WorkspaceSettingsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset to Defaults' }));

    expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeDisabled();

    resolveReset(mockTheme);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reset to Defaults' })).toBeEnabled();
    });
  });

  // AI Settings Tests
  describe('AI Settings', () => {
    const openAiTab = async (user: ReturnType<typeof userEvent.setup>) => {
      const aiTab = await screen.findByRole('tab', { name: /AI Settings/i });
      await user.click(aiTab);
    };

    const enableOverride = async (user: ReturnType<typeof userEvent.setup>) => {
      const overrideCheckbox = await screen.findByRole('checkbox', {
        name: /Override organization AI settings/i,
      });
      await user.click(overrideCheckbox);
    };

    it('renders AI Provider Settings section', async () => {
      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('AI Provider Settings')).toBeInTheDocument();
      });
    });

    it('renders override checkbox', async () => {
      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('Override organization AI settings')).toBeInTheDocument();
      });
      const override = screen.getByRole('checkbox', { name: 'Override organization AI settings' });
      const hint = document.getElementById(override.getAttribute('aria-describedby') ?? '');
      expect(hint?.textContent).toBe(
        "When disabled, this workspace uses the organization's AI provider settings."
      );
      expect(override.closest('.toggle-row')).not.toBeNull();
    });

    it('shows effective settings when not overriding', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);

      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText(/Effective Settings/)).toBeInTheDocument();
      });
    });

    it('displays effective provider value', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);

      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('Self-Hosted (Ollama via LiteLLM)')).toBeInTheDocument();
      });
    });

    it('shows provider form when override is enabled', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);

      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('Override organization AI settings')).toBeInTheDocument();
      });

      await enableOverride(user);

      await waitFor(() => {
        expect(screen.getByLabelText('AI Provider')).toBeInTheDocument();
      });
    });

    it('shows credential fields for selected provider when overriding', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);

      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('Override organization AI settings')).toBeInTheDocument();
      });

      await enableOverride(user);

      await waitFor(() => {
        expect(screen.getByLabelText(/LiteLLM Host/i)).toBeInTheDocument();
      });
    });

    it('changes provider when dropdown changes', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);

      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('Override organization AI settings')).toBeInTheDocument();
      });

      await enableOverride(user);

      await waitFor(() => {
        expect(screen.getByLabelText('AI Provider')).toBeInTheDocument();
      });

      await user.selectOptions(screen.getByLabelText('AI Provider'), 'openai');

      await waitFor(() => {
        // Look for OpenAI-specific content (model options change to OpenAI models)
        expect(screen.getByText(/OpenAI API Key/i)).toBeInTheDocument();
      });
    });

    it('shows model selection when overriding', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);

      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);

      await openAiTab(user);

      await waitFor(() => {
        expect(screen.getByText('Override organization AI settings')).toBeInTheDocument();
      });

      await enableOverride(user);

      await waitFor(() => {
        expect(screen.getByText('Default Models')).toBeInTheDocument();
        expect(screen.getByLabelText('Fast Model')).toBeInTheDocument();
        expect(screen.getByLabelText('Reasoning Model')).toBeInTheDocument();
        expect(screen.getByLabelText('Embedding Model')).toBeInTheDocument();
      });
    });

    it('keeps the override of a workspace that saved the self-hosted provider', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue({
        ...inheritedAiSettings,
        overrides: true,
      });
      mockClient.getEffectiveAiSettings.mockResolvedValue(inheritedAiSettings);
      render(<WorkspaceSettingsPage />);
      await openAiTab(userEvent.setup());

      expect(await screen.findByLabelText('AI Provider')).toHaveValue('self_hosted');
      expect(
        screen.getByRole('checkbox', { name: 'Override organization AI settings' })
      ).toBeChecked();
      expect(screen.queryByText(/Effective Settings/)).toBeNull();
    });

    it('drops the override when it is switched off and saved', async () => {
      const organizationSettings: AiSettings = { ...mockAiSettings, provider: 'claude_code' };
      mockClient.getEffectiveAiSettings
        .mockResolvedValueOnce(savedAiSettings)
        .mockResolvedValueOnce(organizationSettings);
      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);
      await openAiTab(user);

      await user.click(
        await screen.findByRole('checkbox', { name: 'Override organization AI settings' })
      );
      await user.click(screen.getByRole('button', { name: 'Save Changes' }));

      expect(await screen.findByText('Settings saved successfully')).toBeInTheDocument();
      expect(mockClient.resetWorkspaceAiSettings).toHaveBeenCalledWith(
        '00000000-0000-0000-0000-000000000001',
        '00000000-0000-0000-0000-000000000001'
      );
      expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
      expect(mockClient.getEffectiveAiSettings).toHaveBeenCalledTimes(2);
      expect(await screen.findByText('Claude Code (Claude subscription)')).toBeInTheDocument();
      expect(
        screen.getByRole('checkbox', { name: 'Override organization AI settings' })
      ).not.toBeChecked();
    });

    it('sends nothing when a workspace that never overrode is saved', async () => {
      mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);
      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);
      await openAiTab(user);
      await screen.findByText(/Effective Settings/);

      await user.click(screen.getByRole('button', { name: 'Save Changes' }));

      expect(await screen.findByText('Settings saved successfully')).toBeInTheDocument();
      expect(mockClient.resetWorkspaceAiSettings).not.toHaveBeenCalled();
      expect(mockClient.updateWorkspaceAiSettings).not.toHaveBeenCalled();
    });

    it('loads AI settings when its tab opens', async () => {
      render(<WorkspaceSettingsPage />);
      await openAiTab(userEvent.setup());
      await waitFor(() => {
        expect(mockClient.getWorkspaceAiSettings).toHaveBeenCalled();
        expect(mockClient.getEffectiveAiSettings).toHaveBeenCalled();
      });
    });

    it('sends an empty video model to resume organization inheritance', async () => {
      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);
      await openAiTab(user);
      await waitFor(() => {
        expect(screen.getByLabelText('Video Model')).toHaveValue('wan2.2_ti2v_5B_fp16.safetensors');
      });
      await user.selectOptions(screen.getByLabelText('Video Model'), '');
      await user.click(screen.getByRole('button', { name: 'Save Changes' }));
      await waitFor(() => {
        expect(mockClient.updateWorkspaceAiSettings).toHaveBeenCalledWith(
          expect.any(String),
          expect.any(String),
          expect.objectContaining({ model_video: '' })
        );
      });
    });

    it('sends an empty audio model to resume organization inheritance', async () => {
      const user = userEvent.setup();
      render(<WorkspaceSettingsPage />);
      await openAiTab(user);
      await waitFor(() => {
        expect(screen.getByLabelText('Audio Model')).toHaveValue('ace_step_v1_3.5b.safetensors');
      });
      await user.selectOptions(screen.getByLabelText('Audio Model'), '');
      await user.click(screen.getByRole('button', { name: 'Save Changes' }));
      await waitFor(() => {
        expect(mockClient.updateWorkspaceAiSettings).toHaveBeenCalledWith(
          expect.any(String),
          expect.any(String),
          expect.objectContaining({ model_audio: '' })
        );
      });
    });

    describe('coding agent overrides', () => {
      const codexOnly: WorkspaceAiSettings = {
        ...inheritedAiSettings,
        provider: 'codex',
        overrides: true,
      };

      it('opens with the override on when the workspace only chose a coding agent', async () => {
        mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
        render(<WorkspaceSettingsPage />);
        await openAiTab(userEvent.setup());

        expect(await screen.findByLabelText('AI Provider')).toHaveValue('codex');
        expect(
          screen.getByRole('checkbox', { name: 'Override organization AI settings' })
        ).toBeChecked();
        expect(await screen.findByText('Codex sign-in')).toBeInTheDocument();
        expect(agentsApi.list).toHaveBeenCalledWith('00000000-0000-0000-0000-000000000001');
        await waitFor(() =>
          expect(
            Array.from(
              (screen.getByLabelText('Fast Model') as HTMLSelectElement).options,
              (option) => option.value
            )
          ).toEqual(['', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna'])
        );
      });

      it('does not ask for sign-in status while inheriting the organization', async () => {
        mockClient.getWorkspaceAiSettings.mockResolvedValue(inheritedAiSettings);
        mockClient.getEffectiveAiSettings.mockResolvedValue({ ...codexOnly, provider: 'codex' });
        render(<WorkspaceSettingsPage />);
        await openAiTab(userEvent.setup());

        expect(await screen.findByText('Codex (ChatGPT subscription)')).toBeInTheDocument();
        expect(agentsApi.list).not.toHaveBeenCalled();
      });

      it('shows a member the organization sign-in with no code and no buttons', async () => {
        organizationRole = 'member';
        mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
        render(<WorkspaceSettingsPage />);
        await openAiTab(userEvent.setup());

        const panel = (await screen.findByText('Codex sign-in')).closest('section');
        expect(panel).not.toBeNull();
        expect(
          await screen.findByText('Ask an organization admin to sign in.')
        ).toBeInTheDocument();
        expect(panel?.querySelectorAll('button')).toHaveLength(0);
        expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
      });

      describe('after the provider changes', () => {
        const [claudeStatus, codexStatus] = fixture.agents;
        const signedOut = [
          { ...claudeStatus, state: 'signed_out', source: null, label: null, expires_at: null },
          { ...codexStatus, state: 'signed_out', pending: null },
        ];
        const later = () => new Date(Date.now() + 10 * 60_000).toISOString();

        beforeEach(() => {
          organizationRole = 'owner';
          agentsApi.list.mockResolvedValue(signedOut);
        });

        it("shows Claude none of Codex's sign-in", async () => {
          mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
          agentsApi.start.mockResolvedValue({
            agent: 'codex',
            verification_url: 'https://auth.openai.com/codex/device',
            user_code: 'ABCD-EFGHI',
            expires_at: later(),
          });
          agentsApi.signOut.mockRejectedValue(new Error('Failed to sign out of codex: 500'));
          const user = userEvent.setup();
          render(<WorkspaceSettingsPage />);
          await openAiTab(user);

          await user.click(await screen.findByRole('button', { name: 'Sign in with ChatGPT' }));
          expect(await screen.findByText('ABCD-EFGHI')).toBeInTheDocument();
          await user.click(screen.getByRole('button', { name: 'Cancel' }));
          expect(await screen.findByRole('alert')).toHaveTextContent(
            'Failed to sign out of codex: 500'
          );

          await user.selectOptions(screen.getByLabelText('AI Provider'), 'claude_code');

          const panel = screen.getByRole('region', { name: 'Claude Code sign-in' });
          expect(within(panel).getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
          expect(within(panel).getByText('Not signed in')).toBeInTheDocument();
          expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
          expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
          expect(screen.queryByRole('alert')).toBeNull();
        });

        it("shows Codex none of Claude's sign-in", async () => {
          mockClient.getWorkspaceAiSettings.mockResolvedValue({
            ...codexOnly,
            provider: 'claude_code',
          });
          agentsApi.start.mockResolvedValue({
            agent: 'claude',
            authorize_url: 'https://claude.com/cai/oauth/authorize?code=true&state=fake-state',
            expires_at: later(),
          });
          agentsApi.submitCode.mockRejectedValue(
            new Error('Claude rejected the code: Invalid authorization code')
          );
          const user = userEvent.setup();
          render(<WorkspaceSettingsPage />);
          await openAiTab(user);

          await user.click(await screen.findByRole('button', { name: 'Sign in with Claude' }));
          await user.type(
            await screen.findByLabelText('Code from claude.com'),
            'fake-code#fake-state'
          );
          await user.click(screen.getByRole('button', { name: 'Submit code' }));
          expect(await screen.findByRole('alert')).toHaveTextContent('Claude rejected the code');

          await user.selectOptions(screen.getByLabelText('AI Provider'), 'codex');

          const panel = screen.getByRole('region', { name: 'Codex sign-in' });
          expect(within(panel).getByRole('button', { name: 'Sign in with ChatGPT' })).toBeEnabled();
          expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
          expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
          expect(screen.queryByRole('alert')).toBeNull();
        });
      });

      it('shows the sign-in status alone while the role resolves, then fails closed', async () => {
        resolvingRole = true;
        mockClient.getWorkspaceAiSettings.mockResolvedValue({
          ...codexOnly,
          provider: 'claude_code',
        });
        agentsApi.list.mockResolvedValue([
          {
            ...fixture.agents[0],
            state: 'signed_out',
            source: null,
            label: null,
            expires_at: null,
          },
          fixture.agents[1],
        ]);
        const { rerender } = render(<WorkspaceSettingsPage />);
        await openAiTab(userEvent.setup());

        const panel = await screen.findByRole('region', { name: 'Claude Code sign-in' });
        expect(await within(panel).findByText('Not signed in')).toBeInTheDocument();
        expect(within(panel).queryAllByRole('button')).toHaveLength(0);
        expect(within(panel).queryByText('Ask an organization admin to sign in.')).toBeNull();

        resolvingRole = false;
        rerender(<WorkspaceSettingsPage />);

        expect(
          within(panel).getByText('Ask an organization admin to sign in.')
        ).toBeInTheDocument();
        expect(within(panel).queryAllByRole('button')).toHaveLength(0);
      });

      it('saves the override without credentials', async () => {
        mockClient.getWorkspaceAiSettings.mockResolvedValue(codexOnly);
        mockClient.updateWorkspaceAiSettings.mockResolvedValue(codexOnly);
        const user = userEvent.setup();
        render(<WorkspaceSettingsPage />);
        await openAiTab(user);
        await screen.findByText('Codex sign-in');

        await user.click(screen.getByRole('button', { name: 'Save Changes' }));

        await waitFor(() => expect(mockClient.updateWorkspaceAiSettings).toHaveBeenCalled());
        const [, , request] = mockClient.updateWorkspaceAiSettings.mock.calls[0];
        expect(request).toEqual({
          provider: 'codex',
          model_fast: '',
          model_reasoning: '',
          model_embedding: '',
          model_image: '',
          model_video: '',
          model_audio: '',
        });
      });

      it('clears the models of the provider it switched away from', async () => {
        mockClient.updateWorkspaceAiSettings.mockResolvedValue(codexOnly);
        const user = userEvent.setup();
        render(<WorkspaceSettingsPage />);
        await openAiTab(user);
        await waitFor(() => expect(screen.getByLabelText('Fast Model')).toHaveValue('llama3.1:8b'));

        await user.selectOptions(screen.getByLabelText('AI Provider'), 'codex');
        await user.click(screen.getByRole('button', { name: 'Save Changes' }));

        await waitFor(() => expect(mockClient.updateWorkspaceAiSettings).toHaveBeenCalled());
        const [, , request] = mockClient.updateWorkspaceAiSettings.mock.calls[0];
        expect(request.provider).toBe('codex');
        expect(request.model_fast).toBe('');
        expect(request.model_reasoning).toBe('');
        expect(request.model_embedding).toBe('');
      });
    });
  });
});
