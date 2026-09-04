import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen } from '@testing-library/react';
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
      window.matchMedia = mock().mockImplementation((query) => ({
        matches: query === '(prefers-color-scheme: dark)',
        media: query,
        addEventListener: mock(),
        removeEventListener: mock(),
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
      window.matchMedia = mock().mockImplementation((query) => ({
        matches: false, // Not dark mode
        media: query,
        addEventListener: mock(),
        removeEventListener: mock(),
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
    it('ignores legacy unscoped workspace themes', () => {
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

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('null');
      expect(localStorage.getItem('manager_workspace_theme')).toBeNull();
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

    it('keeps saved workspace themes scoped to the session', () => {
      localStorage.setItem('manager_theme', 'light');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('null');

      fireEvent.click(screen.getByText('Set Workspace Theme'));

      expect(screen.getByTestId('workspace-theme')).toHaveTextContent('set');
      expect(localStorage.getItem('manager_workspace_theme')).toBeNull();
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
      expect(root.style.getPropertyValue('--ui-accent')).toBe('#ff0000');
      expect(root.style.getPropertyValue('--ui-font-body')).toBe(FONT_MAP.inter);
    });

    it('removes CSS variables when workspace theme is cleared', () => {
      localStorage.setItem('manager_theme', 'light');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      fireEvent.click(screen.getByText('Set Workspace Theme'));
      expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('#ff0000');

      fireEvent.click(screen.getByText('Clear Workspace Theme'));
      expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('');
    });

    it('uses dark colors when in dark mode', () => {
      localStorage.setItem('manager_theme', 'dark');

      render(
        <ThemeProvider>
          <TestComponent />
        </ThemeProvider>
      );

      fireEvent.click(screen.getByText('Set Workspace Theme'));

      expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('#cc0000');
    });
  });

  describe('useTheme hook', () => {
    it('throws error when used outside provider', () => {
      const consoleError = console.error;
      console.error = mock();

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

describe('Workspace theme overrides', () => {
  let context: ReturnType<typeof useTheme>;
  function Capture() {
    context = useTheme();
    return null;
  }
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem('manager_theme', 'light');
    document.documentElement.style.cssText = '';
    render(
      <ThemeProvider>
        <Capture />
      </ThemeProvider>
    );
  });

  it('previews without changing saved state and restores saved appearance', () => {
    const saved = { primary_color_light: '#123' } as WorkspaceTheme;
    act(() => context.setWorkspaceTheme(saved));
    act(() => context.previewWorkspaceTheme({ primary_color_light: '#fff' } as WorkspaceTheme));
    expect(context.workspaceTheme).toEqual(saved);
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('#ffffff');
    expect(document.documentElement.style.getPropertyValue('--ui-accent-foreground')).toBe(
      '#000000'
    );
    act(() => context.previewWorkspaceTheme(null));
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('#112233');
    expect(document.documentElement.style.getPropertyValue('--ui-accent-foreground')).toBe(
      '#ffffff'
    );
  });

  it('applies secondary colors, both fonts, rem scaling and all radius sizes', () => {
    act(() =>
      context.setWorkspaceTheme({
        secondary_color_light: '#abc',
        font_family: 'nunito',
        font_size_base: '18px',
        border_radius: 'none',
      } as WorkspaceTheme)
    );
    const style = document.documentElement.style;
    expect(style.getPropertyValue('--ui-secondary')).toBe('#aabbcc');
    expect(style.getPropertyValue('--ui-secondary-foreground')).toBe('#000000');
    expect(style.getPropertyValue('--ui-font-body')).toBe(FONT_MAP.nunito);
    expect(style.getPropertyValue('--ui-font-display')).toBe(FONT_MAP.nunito);
    expect(style.fontSize).toBe('18px');
    for (const size of ['sm', 'md', 'lg', 'xl', '2xl', '3xl'])
      expect(style.getPropertyValue(`--ui-radius-${size}`)).toBe('0');
    act(() => context.setWorkspaceTheme(null));
    expect(style.cssText).toBe('');
  });

  it('restores native defaults for null and invalid stored values', () => {
    act(() =>
      context.setWorkspaceTheme({
        primary_color_light: '#123',
        font_family: 'inter',
        border_radius: 'small',
      } as WorkspaceTheme)
    );
    act(() =>
      context.setWorkspaceTheme({
        primary_color_light: 'red; color: black',
        secondary_color_light: null,
        font_family: 'missing',
        font_size_base: '-5px',
        border_radius: 'missing',
      } as unknown as WorkspaceTheme)
    );
    expect(document.documentElement.style.cssText).toBe('');
  });
});
