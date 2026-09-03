import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

const mockGenerateSecret = mock(() => 'generated-secret');
const mockInstall = mock();
const mockResetInstall = mock();
const mockUseKeyboardNavigation = mock();
const mockLoadConfig = mock(() => Promise.resolve(null));
const mockSaveConfig = mock(() => Promise.resolve(undefined));
const mockClearConfig = mock();
const passSchema = { safeParse: () => ({ success: true, data: {} }) };

mock.module('../hooks', () => ({
  useSecretGenerator: () => ({
    generateSecret: mockGenerateSecret,
  }),
}));

mock.module('../hooks/useInstallation', () => ({
  useInstallation: () => ({
    isInstalling: false,
    progress: 0,
    statusLines: [],
    isComplete: false,
    error: null,
    install: mockInstall,
    reset: mockResetInstall,
  }),
}));

mock.module('../hooks/useKeyboardNavigation', () => ({
  useKeyboardNavigation: mockUseKeyboardNavigation,
}));

mock.module('../utils/crypto', () => ({
  loadConfig: mockLoadConfig,
  saveConfig: mockSaveConfig,
  clearConfig: mockClearConfig,
  SENSITIVE_FIELDS: [],
}));

mock.module('../validation/schemas', () => ({
  StepSchemas: {
    domain: passSchema,
    security: passSchema,
    models: passSchema,
    interface: passSchema,
    search: passSchema,
    vpn: passSchema,
    advanced: passSchema,
  },
}));

mock.module('@hookform/resolvers/zod', () => ({
  zodResolver: () => async () => ({ values: {}, errors: {} }),
}));

let InstallerForm: typeof import('./InstallerForm').default;

beforeAll(async () => {
  InstallerForm = (await import('./InstallerForm')).default;
});

afterAll(() => {
  mock.restore();
});

describe('InstallerForm', () => {
  beforeEach(() => {
    mockGenerateSecret.mockReset();
    mockGenerateSecret.mockReturnValue('generated-secret');
    mockInstall.mockReset();
    mockResetInstall.mockReset();
    mockUseKeyboardNavigation.mockReset();
    mockLoadConfig.mockReset();
    mockLoadConfig.mockResolvedValue(null);
    mockSaveConfig.mockReset();
    mockSaveConfig.mockResolvedValue(undefined);
    mockClearConfig.mockReset();
  });

  it('renders the installer layout', () => {
    render(<InstallerForm />);

    expect(screen.getByText('Zone')).toBeInTheDocument();
    expect(screen.getByText('Configuration')).toBeInTheDocument();
  });

  it('renders first step (Domain) by default', () => {
    render(<InstallerForm />);

    expect(screen.getByRole('heading', { name: 'Domain Configuration' })).toBeInTheDocument();
  });

  it('renders navigation buttons', () => {
    render(<InstallerForm />);

    expect(screen.getByRole('button', { name: /previous/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /next/i })).toBeInTheDocument();
  });

  it('disables Previous button on first step', () => {
    render(<InstallerForm />);

    expect(screen.getByRole('button', { name: /previous/i })).toBeDisabled();
  });

  it('navigates to next step when Next is clicked', async () => {
    render(<InstallerForm />);

    fireEvent.click(screen.getByRole('button', { name: /next/i }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Security' })).toBeInTheDocument();
    });
  });

  it('navigates to previous step when Previous is clicked', async () => {
    render(<InstallerForm />);

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Security' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /previous/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Domain Configuration' })).toBeInTheDocument();
    });
  });

  it('enables Previous button after navigating past first step', async () => {
    render(<InstallerForm />);

    fireEvent.click(screen.getByRole('button', { name: /next/i }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /previous/i })).not.toBeDisabled();
    });
  });

  it('shows Install button on last step', async () => {
    render(<InstallerForm />);

    for (let i = 0; i < 6; i++) {
      fireEvent.click(screen.getByRole('button', { name: /next/i }));
    }

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /install/i })).toBeInTheDocument();
    });
  });

  it('renders step pills for navigation', () => {
    render(<InstallerForm />);

    expect(screen.getByRole('navigation', { name: /installation steps/i })).toBeInTheDocument();
  });

  it('navigates to clicked step in step pills', async () => {
    render(<InstallerForm />);

    fireEvent.click(screen.getByText('Models'));

    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: 'AI Provider Configuration' })
      ).toBeInTheDocument();
    });
  });

  it('renders all steps when navigating', async () => {
    render(<InstallerForm />);

    expect(screen.getByRole('heading', { name: 'Domain Configuration' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Security' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: 'AI Provider Configuration' })
      ).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Interface Settings' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Web Search' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'VPN Configuration' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Advanced Settings' })).toBeInTheDocument();
    });
  });

  it('renders sidebar with Zone header', () => {
    render(<InstallerForm />);

    const sidebar = screen.getByTestId('installer-sidebar');
    expect(sidebar).toBeInTheDocument();
    expect(sidebar).toHaveTextContent('Zone');
  });

  it('renders main content area with card', () => {
    render(<InstallerForm />);

    expect(screen.getByTestId('installer-main')).toBeInTheDocument();
    expect(screen.getByTestId('installer-card')).toBeInTheDocument();
  });
});
