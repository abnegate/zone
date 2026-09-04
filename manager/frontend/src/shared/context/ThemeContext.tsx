import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { client } from '../../api/client';
import type { BorderRadius, FontFamily, WorkspaceTheme } from '../../types';

type Theme = 'light' | 'dark';

interface ThemeContextType {
  theme: Theme;
  toggleTheme: () => void;
  workspaceTheme: WorkspaceTheme | null;
  workspaceThemeLoading: boolean;
  workspaceThemeError: string | null;
  setWorkspaceTheme: (theme: WorkspaceTheme | null) => void;
  previewWorkspaceTheme: (theme: WorkspaceTheme | null) => void;
  loadWorkspaceTheme: (organization: string | null, workspace: string | null) => () => void;
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined);
const THEME_STORAGE_KEY = 'manager_theme';
const WORKSPACE_THEME_STORAGE_KEY = 'manager_workspace_theme';

const FONT_MAP: Record<FontFamily, string> = {
  system: 'system-ui, -apple-system, BlinkMacSystemFont, sans-serif',
  inter: '"Inter", system-ui, sans-serif',
  roboto: '"Roboto", system-ui, sans-serif',
  'open-sans': '"Open Sans", system-ui, sans-serif',
  lato: '"Lato", system-ui, sans-serif',
  nunito: '"Nunito", system-ui, sans-serif',
};

const RADIUS_MAP: Record<BorderRadius, Record<string, string>> = {
  none: { sm: '0', md: '0', lg: '0', xl: '0', '2xl': '0', '3xl': '0' },
  small: {
    sm: '0.125rem',
    md: '0.25rem',
    lg: '0.375rem',
    xl: '0.5rem',
    '2xl': '0.75rem',
    '3xl': '1rem',
  },
  medium: {
    sm: '0.375rem',
    md: '0.5rem',
    lg: '0.75rem',
    xl: '1rem',
    '2xl': '1.25rem',
    '3xl': '1.5rem',
  },
  large: { sm: '0.5rem', md: '0.75rem', lg: '1rem', xl: '1.5rem', '2xl': '2rem', '3xl': '3rem' },
};

function color(value: string | null | undefined): string | null {
  if (!value || !/^#(?:[\da-f]{3}|[\da-f]{6})$/i.test(value)) return null;
  return value.length === 4
    ? `#${value
        .slice(1)
        .split('')
        .map((digit) => digit + digit)
        .join('')}`.toLowerCase()
    : value.toLowerCase();
}

function mix(value: string, target: number, amount: number): string {
  return `#${[1, 3, 5]
    .map((offset) => {
      const channel = Number.parseInt(value.slice(offset, offset + 2), 16);
      return Math.round(channel + (target - channel) * amount)
        .toString(16)
        .padStart(2, '0');
    })
    .join('')}`;
}

function foreground(value: string): string {
  const channels = [1, 3, 5].map((offset) => {
    const channel = Number.parseInt(value.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  const luminance = channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  return luminance > 0.179 ? '#000000' : '#ffffff';
}

function applyWorkspaceTheme(workspace: WorkspaceTheme | null, mode: Theme): () => void {
  const root = document.documentElement;
  const properties = new Map<string, string>();
  const primary = color(
    mode === 'dark' ? workspace?.primary_color_dark : workspace?.primary_color_light
  );
  const secondary = color(
    mode === 'dark' ? workspace?.secondary_color_dark : workspace?.secondary_color_light
  );

  if (primary) {
    properties.set('--ui-accent', primary);
    properties.set(
      '--ui-accent-hover',
      mix(primary, foreground(primary) === '#ffffff' ? 0 : 255, 0.15)
    );
    properties.set('--ui-accent-foreground', foreground(primary));
    properties.set('--ui-border-focus', primary);
    properties.set('--ui-accent-muted', `${primary}24`);
    properties.set('--ui-accent-glow', `${primary}38`);
    for (const shade of [50, 100, 200, 300, 400, 500, 600, 700, 800, 900]) {
      const amount = (shade - 600) / 600;
      properties.set(
        `--ui-accent-${shade}`,
        mix(primary, shade < 600 !== (mode === 'dark') ? 255 : 0, Math.abs(amount))
      );
    }
  }
  if (secondary) {
    properties.set('--ui-secondary', secondary);
    properties.set(
      '--ui-secondary-hover',
      mix(secondary, foreground(secondary) === '#ffffff' ? 0 : 255, 0.15)
    );
    properties.set('--ui-secondary-foreground', foreground(secondary));
    properties.set('--ui-secondary-400', mix(secondary, 255, 0.15));
    properties.set('--ui-secondary-500', secondary);
    properties.set('--ui-secondary-600', mix(secondary, 0, 0.15));
  }
  if (workspace?.font_family && Object.hasOwn(FONT_MAP, workspace.font_family)) {
    properties.set('--ui-font-body', FONT_MAP[workspace.font_family]);
    properties.set('--ui-font-display', FONT_MAP[workspace.font_family]);
  }
  if (
    workspace?.font_size_base &&
    /^\d+(?:\.\d+)?px$/.test(workspace.font_size_base) &&
    Number.isFinite(Number.parseFloat(workspace.font_size_base)) &&
    Number.parseFloat(workspace.font_size_base) > 0
  ) {
    properties.set('font-size', workspace.font_size_base);
  }
  if (workspace?.border_radius && Object.hasOwn(RADIUS_MAP, workspace.border_radius)) {
    for (const [size, radius] of Object.entries(RADIUS_MAP[workspace.border_radius])) {
      properties.set(`--ui-radius-${size}`, radius);
    }
  }
  const previous = new Map<string, string>();
  for (const [name, value] of properties) {
    previous.set(name, root.style.getPropertyValue(name));
    root.style.setProperty(name, value);
  }
  return () => {
    for (const [name, value] of previous) {
      if (value) root.style.setProperty(name, value);
      else root.style.removeProperty(name);
    }
  };
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    return stored === 'light' || stored === 'dark'
      ? stored
      : window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light';
  });
  const [workspaceTheme, setWorkspaceThemeState] = useState<WorkspaceTheme | null>(null);
  const [preview, setPreview] = useState<WorkspaceTheme | null>(null);
  const [workspaceThemeLoading, setLoading] = useState(false);
  const [workspaceThemeError, setError] = useState<string | null>(null);
  const revision = useRef(0);

  const toggleTheme = useCallback(() => {
    setTheme((previous) => {
      const next = previous === 'dark' ? 'light' : 'dark';
      localStorage.setItem(THEME_STORAGE_KEY, next);
      return next;
    });
  }, []);

  const setWorkspaceTheme = useCallback((value: WorkspaceTheme | null) => {
    revision.current += 1;
    setWorkspaceThemeState(value);
    setPreview(null);
    setLoading(false);
    setError(null);
  }, []);

  const loadWorkspaceTheme = useCallback(
    (organization: string | null, workspace: string | null) => {
      const request = ++revision.current;
      setWorkspaceThemeState(null);
      setPreview(null);
      setError(null);
      setLoading(Boolean(organization && workspace));
      if (organization && workspace) {
        client
          .getWorkspaceTheme(organization, workspace)
          .then((value) => {
            if (revision.current !== request) return;
            setWorkspaceThemeState(value);
            setLoading(false);
          })
          .catch((error: unknown) => {
            if (revision.current !== request) return;
            setError(error instanceof Error ? error.message : 'Failed to load workspace theme');
            setLoading(false);
          });
      }
      return () => {
        revision.current += 1;
      };
    },
    []
  );

  useEffect(() => {
    localStorage.removeItem(WORKSPACE_THEME_STORAGE_KEY);
  }, []);

  useLayoutEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    return applyWorkspaceTheme(preview ?? workspaceTheme, theme);
  }, [preview, workspaceTheme, theme]);

  return (
    <ThemeContext.Provider
      value={{
        theme,
        toggleTheme,
        workspaceTheme,
        workspaceThemeLoading,
        workspaceThemeError,
        setWorkspaceTheme,
        previewWorkspaceTheme: setPreview,
        loadWorkspaceTheme,
      }}
    >
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextType {
  const context = useContext(ThemeContext);
  if (context === undefined) throw new Error('useTheme must be used within a ThemeProvider');
  return context;
}

export { FONT_MAP, RADIUS_MAP };
