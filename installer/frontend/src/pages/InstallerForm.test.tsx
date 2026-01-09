import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import InstallerForm from './InstallerForm';

// Mock the hooks
jest.mock('../hooks', () => ({
  useSecretGenerator: () => ({
    generateSecret: jest.fn().mockReturnValue('generated-secret'),
  }),
  useInstallation: () => ({
    isInstalling: false,
    progress: 0,
    statusLines: [],
    isComplete: false,
    error: null,
    install: jest.fn(),
    reset: jest.fn(),
  }),
  useValidation: () => ({
    validateStep: jest.fn().mockReturnValue(true),
    getFieldError: jest.fn().mockReturnValue(undefined),
    clearErrors: jest.fn(),
  }),
  useConfigPersistence: jest.fn().mockReturnValue({ resetConfig: jest.fn() }),
  useKeyboardNavigation: jest.fn(),
}));

// Mock crypto utilities
jest.mock('../utils/crypto', () => ({
  loadConfig: jest.fn().mockResolvedValue(null),
  saveConfig: jest.fn().mockResolvedValue(undefined),
  clearConfig: jest.fn(),
  SENSITIVE_FIELDS: [],
}));

describe('InstallerForm', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders the installer layout', () => {
    render(<InstallerForm />);

    expect(screen.getByText('Zone')).toBeInTheDocument();
    expect(screen.getByText('Configuration')).toBeInTheDocument();
  });

  it('renders first step (Domain) by default', () => {
    render(<InstallerForm />);

    expect(screen.getByText('Domain Configuration')).toBeInTheDocument();
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
      expect(screen.getByRole('heading', { level: 2, name: 'Security' })).toBeInTheDocument();
    });
  });

  it('navigates to previous step when Previous is clicked', async () => {
    render(<InstallerForm />);

    // Go to step 2
    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 2, name: 'Security' })).toBeInTheDocument();
    });

    // Go back to step 1
    fireEvent.click(screen.getByRole('button', { name: /previous/i }));
    await waitFor(() => {
      expect(screen.getByText('Domain Configuration')).toBeInTheDocument();
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

    // Navigate through all steps
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

    // Click on Models step (step 3)
    fireEvent.click(screen.getByText('Models'));

    await waitFor(() => {
      expect(screen.getByText('AI Provider Configuration')).toBeInTheDocument();
    });
  });

  it('renders all steps when navigating', async () => {
    render(<InstallerForm />);

    // Step 1: Domain
    expect(screen.getByText('Domain Configuration')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 2, name: 'Security' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { level: 2, name: 'AI Provider Configuration' })
      ).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { level: 2, name: 'Interface Settings' })
      ).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 2, name: 'Web Search' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { level: 2, name: 'VPN Configuration' })
      ).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /next/i }));
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { level: 2, name: 'Advanced Settings' })
      ).toBeInTheDocument();
    });
  });

  it('renders sidebar with Zone header', () => {
    render(<InstallerForm />);

    const sidebar = document.querySelector('.installer-sidebar');
    expect(sidebar).toBeInTheDocument();
    expect(sidebar?.querySelector('h1')).toHaveTextContent('Zone');
  });

  it('renders main content area with card', () => {
    render(<InstallerForm />);

    expect(document.querySelector('.installer-main')).toBeInTheDocument();
    expect(document.querySelector('.card')).toBeInTheDocument();
  });
});
