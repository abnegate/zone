import { createContext, type ReactNode, useCallback, useContext, useEffect, useState } from 'react';
import type { BorderRadius, FontFamily, WorkspaceTheme } from '../../types';

type Theme = 'light' | 'dark';

interface ThemeContextType {
  theme: Theme;
  toggleTheme: () => void;
  workspaceTheme: WorkspaceTheme | null;
  setWorkspaceTheme: (theme: WorkspaceTheme | null) => void;
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined);

const THEME_STORAGE_KEY = 'manager_theme';
const WORKSPACE_THEME_STORAGE_KEY = 'manager_workspace_theme';

// Font family mappings
const FONT_MAP: Record<FontFamily, string> = {
  system: 'system-ui, -apple-system, BlinkMacSystemFont, sans-serif',
  inter: '"Inter", system-ui, sans-serif',
  roboto: '"Roboto", system-ui, sans-serif',
  'open-sans': '"Open Sans", system-ui, sans-serif',
  lato: '"Lato", system-ui, sans-serif',
  nunito: '"Nunito", system-ui, sans-serif',
};

// Border radius mappings
const RADIUS_MAP: Record<BorderRadius, { sm: string; md: string; lg: string }> = {
  none: { sm: '0', md: '0', lg: '0' },
  small: { sm: '0.125rem', md: '0.25rem', lg: '0.375rem' },
  medium: { sm: '0.375rem', md: '0.5rem', lg: '0.75rem' },
  large: { sm: '0.5rem', md: '0.75rem', lg: '1rem' },
};

function getSystemTheme(): Theme {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

// Darken a hex color by a percentage
function darkenColor(hex: string, percent: number): string {
  const num = Number.parseInt(hex.replace('#', ''), 16);
  const amt = Math.round(2.55 * percent);
  const R = Math.max((num >> 16) - amt, 0);
  const G = Math.max(((num >> 8) & 0x00ff) - amt, 0);
  const B = Math.max((num & 0x0000ff) - amt, 0);
  return `#${(0x1000000 + R * 0x10000 + G * 0x100 + B).toString(16).slice(1)}`;
}

function applyWorkspaceTheme(workspaceTheme: WorkspaceTheme | null, mode: Theme) {
  const root = document.documentElement;

  if (!workspaceTheme) {
    // Clear custom properties, let CSS defaults take over
    root.style.removeProperty('--color-primary');
    root.style.removeProperty('--color-primary-hover');
    root.style.removeProperty('--color-secondary');
    root.style.removeProperty('--font-family');
    root.style.removeProperty('--font-size-base');
    root.style.removeProperty('--radius-sm');
    root.style.removeProperty('--radius-md');
    root.style.removeProperty('--radius-lg');
    return;
  }

  // Apply colors based on current mode
  const primary =
    mode === 'dark' ? workspaceTheme.primary_color_dark : workspaceTheme.primary_color_light;
  const secondary =
    mode === 'dark' ? workspaceTheme.secondary_color_dark : workspaceTheme.secondary_color_light;

  root.style.setProperty('--color-primary', primary);
  root.style.setProperty('--color-primary-hover', darkenColor(primary, 10));
  root.style.setProperty('--color-secondary', secondary);

  // Apply font
  root.style.setProperty('--font-family', FONT_MAP[workspaceTheme.font_family]);
  root.style.setProperty('--font-size-base', workspaceTheme.font_size_base);

  // Apply border radius
  const radius = RADIUS_MAP[workspaceTheme.border_radius];
  root.style.setProperty('--radius-sm', radius.sm);
  root.style.setProperty('--radius-md', radius.md);
  root.style.setProperty('--radius-lg', radius.lg);
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === 'light' || stored === 'dark') {
      return stored;
    }
    return getSystemTheme();
  });

  const [workspaceTheme, setWorkspaceThemeState] = useState<WorkspaceTheme | null>(() => {
    try {
      const stored = localStorage.getItem(WORKSPACE_THEME_STORAGE_KEY);
      if (stored) {
        return JSON.parse(stored);
      }
    } catch {
      // Ignore parse errors
    }
    return null;
  });

  const toggleTheme = useCallback(() => {
    setTheme((prev) => {
      const next = prev === 'dark' ? 'light' : 'dark';
      localStorage.setItem(THEME_STORAGE_KEY, next);
      return next;
    });
  }, []);

  const setWorkspaceTheme = useCallback((newTheme: WorkspaceTheme | null) => {
    setWorkspaceThemeState(newTheme);
    if (newTheme) {
      localStorage.setItem(WORKSPACE_THEME_STORAGE_KEY, JSON.stringify(newTheme));
    } else {
      localStorage.removeItem(WORKSPACE_THEME_STORAGE_KEY);
    }
  }, []);

  // Apply data-theme attribute
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);

  // Apply workspace theme CSS variables
  useEffect(() => {
    applyWorkspaceTheme(workspaceTheme, theme);
  }, [workspaceTheme, theme]);

  return (
    <ThemeContext.Provider value={{ theme, toggleTheme, workspaceTheme, setWorkspaceTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (context === undefined) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}

// Export helpers for external use
export { FONT_MAP, RADIUS_MAP };
