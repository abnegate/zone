import { fireEvent, render, screen } from '@testing-library/react';
import type { WorkspaceTheme } from '../../types';
import { FONT_MAP, RADIUS_MAP, ThemeProvider, useTheme } from './ThemeContext';

// Test component to access context
function TestComponent() {
  const { theme, toggleTheme, workspaceTheme, setWorkspaceTheme } = useTheme();
  return (
    <div>
      <span data-testid="theme">{theme}</span>
      <span data-testid="workspace-theme">{workspaceTheme ? 'set' : 'null'}</span>
      <button onClick={toggleTheme}>Toggle</button>
      <button onClick={() => setWorkspaceTheme(null)}>Clear Workspace Theme</button>
      <button
        onClick={() =>
          setWorkspaceTheme({
            primary_color_light: '#ff0000',
            primary_color_dark: '#cc0000',
            secondary_color_light: '#00ff00',
            secondary_color_dark: '#00cc00',
            font_family: 'inter',
            font_size_base: '16px',
            border_radius: 'medium',
          } as WorkspaceTheme)
        }
      >
        Set Workspace Theme
      </button>
    </div>
  );
}

describe('ThemeContext', () => {
  beforeEach(() => {
    localStorage.clear();
    // Reset document styles
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.style.cssText = '';
  });

  describe('ThemeProvider', () => {
    it('uses stored theme from localStorage', () => {
      localStorage.setItem('manager_theme', 'dark');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('theme')).toHaveTextContent('dark');
    });

    it('uses system theme when no stored theme (dark mode)', () => {
      // Mock matchMedia to return dark mode
      const originalMatchMedia = window.matchMedia;
      window.matchMedia = jest.fn().mockImplementation((query) => ({
        matches: query === '(prefers-color-scheme: dark)',
        media: query,
        addEventListener: jest.fn(),
        removeEventListener: jest.fn(),
      }));

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('theme')).toHaveTextContent('dark');
      window.matchMedia = originalMatchMedia;
    });

    it('uses system theme when no stored theme (light mode)', () => {
      // Mock matchMedia to return light mode
      const originalMatchMedia = window.matchMedia;
      window.matchMedia = jest.fn().mockImplementation((query) => ({
        matches: false, // Not dark mode
        media: query,
        addEventListener: jest.fn(),
        removeEventListener: jest.fn(),
      }));

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('theme')).toHaveTextContent('light');
      window.matchMedia = originalMatchMedia;
    });

    it('toggles theme and persists to localStorage', () => {
      localStorage.setItem('manager_theme', 'light');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('theme')).toHaveTextContent('light');

      fireEvent.click(screen.getByText('Toggle'));

      expect(screen.getByTestId('theme')).toHaveTextContent('dark');
      expect(localStorage.getItem('manager_theme')).toBe('dark');
    });

    it('sets data-theme attribute on document', () => {
      localStorage.setItem('manager_theme', 'dark');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    });
  });

  describe('Workspace Theme', () => {
    it('loads workspace theme from localStorage', () => {
      const storedTheme = {
        primary_color_light: '#007bff',
        primary_color_dark: '#0056b3',
        secondary_color_light: '#6c757d',
        secondary_color_dark: '#545b62',
        font_family: 'roboto',
        font_size_base: '14px',
        border_radius: 'small',
      };
      localStorage.setItem('manager_theme', 'light');
      localStorage.setItem('manager_workspace_theme', JSON.stringify(storedTheme));

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('set');
    });

    it('handles invalid JSON in localStorage', () => {
      localStorage.setItem('manager_theme', 'light');
      localStorage.setItem('manager_workspace_theme', 'invalid json');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('null');
    });

    it('sets workspace theme and persists to localStorage', () => {
      localStorage.setItem('manager_theme', 'light');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('null');

      fireEvent.click(screen.getByText('Set Workspace Theme'));

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('set');
      expect(localStorage.getItem('manager_workspace_theme')).not.toBeNull();
    });

    it('clears workspace theme', () => {
      localStorage.setItem('manager_theme', 'light');
      // Use a properly structured theme object
      localStorage.setItem(
        'manager_workspace_theme',
        JSON.stringify({
          primary_color_light: '#007bff',
          primary_color_dark: '#0056b3',
          secondary_color_light: '#6c757d',
          secondary_color_dark: '#545b62',
          font_family: 'system',
          font_size_base: '16px',
          border_radius: 'medium',
        })
      );

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      fireEvent.click(screen.getByText('Clear Workspace Theme'));

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('null');
      expect(localStorage.getItem('manager_workspace_theme')).toBeNull();
    });

    it('applies CSS variables when workspace theme is set', () => {
      localStorage.setItem('manager_theme', 'light');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      fireEvent.click(screen.getByText('Set Workspace Theme'));

      const root = document.documentElement;
      expect(root.style.getPropertyValue('--color-primary')).toBe('#ff0000');
      expect(root.style.getPropertyValue('--font-family')).toBe(FONT_MAP.inter);
    });

    it('removes CSS variables when workspace theme is cleared', () => {
      localStorage.setItem('manager_theme', 'light');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      fireEvent.click(screen.getByText('Set Workspace Theme'));
      expect(document.documentElement.style.getPropertyValue('--color-primary')).toBe('#ff0000');

      fireEvent.click(screen.getByText('Clear Workspace Theme'));
      expect(document.documentElement.style.getPropertyValue('--color-primary')).toBe('');
    });

    it('uses dark colors when in dark mode', () => {
      localStorage.setItem('manager_theme', 'dark');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      fireEvent.click(screen.getByText('Set Workspace Theme'));

      expect(document.documentElement.style.getPropertyValue('--color-primary')).toBe('#cc0000');
    });
  });

  describe('useTheme hook', () => {
    it('throws error when used outside provider', () => {
      const consoleError = console.error;
      console.error = jest.fn();

      expect(() => {
        render(<TestComponent />);
      }).toThrow('useTheme must be used within a ThemeProvider');

      console.error = consoleError;
    });
  });

  describe('FONT_MAP', () => {
    it('contains expected font families', () => {
      expect(FONT_MAP.system).toContain('system-ui');
      expect(FONT_MAP.inter).toContain('Inter');
      expect(FONT_MAP.roboto).toContain('Roboto');
      expect(FONT_MAP['open-sans']).toContain('Open Sans');
      expect(FONT_MAP.lato).toContain('Lato');
      expect(FONT_MAP.nunito).toContain('Nunito');
    });
  });

  describe('RADIUS_MAP', () => {
    it('contains expected border radius values', () => {
      expect(RADIUS_MAP.none.sm).toBe('0');
      expect(RADIUS_MAP.small.md).toBe('0.25rem');
      expect(RADIUS_MAP.medium.lg).toBe('0.75rem');
      expect(RADIUS_MAP.large.lg).toBe('1rem');
    });
  });
});
