import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { act, render } from '@testing-library/react';
import { client } from '../../api/client';
import type { useAuth } from '../../features/auth';
import type { WorkspaceTheme as Configuration } from '../../types';
import { ThemeProvider, useTheme } from './ThemeContext';
import type { useWorkspace } from './WorkspaceContext';
import { WorkspaceTheme } from './WorkspaceTheme';

const saved = (workspace: string, primary: string): Configuration =>
  ({ workspace_id: workspace, primary_color_light: primary }) as Configuration;

describe('WorkspaceTheme synchronization', () => {
  let context: ReturnType<typeof useTheme>;
  let authenticated = true;
  let workspace = 'a';
  const requests = new Map<string, (value: Configuration | null) => void>();
  const useAuthHook = (): ReturnType<typeof useAuth> =>
    ({ isAuthenticated: authenticated, isLoading: false }) as ReturnType<typeof useAuth>;
  const useWorkspaceHook = (): ReturnType<typeof useWorkspace> =>
    ({
      currentOrganization: { id: 'organization' },
      currentWorkspace: { id: workspace },
    }) as ReturnType<typeof useWorkspace>;
  function Capture() {
    context = useTheme();
    return null;
  }
  function Application() {
    return (
      <ThemeProvider>
        <WorkspaceTheme useAuthHook={useAuthHook} useWorkspaceHook={useWorkspaceHook} />
        <Capture />
      </ThemeProvider>
    );
  }
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem('manager_theme', 'light');
    document.documentElement.style.cssText = '';
    authenticated = true;
    workspace = 'a';
    requests.clear();
    spyOn(client, 'getWorkspaceTheme').mockImplementation(
      (_organization, identifier) =>
        new Promise((resolve) => {
          requests.set(identifier, resolve);
        })
    );
  });
  afterEach(() => {
    (client.getWorkspaceTheme as ReturnType<typeof spyOn>).mockRestore();
  });

  it('loads without Settings and ignores obsolete responses after a switch or logout', async () => {
    const application = render(<Application />);
    expect(context.workspaceThemeLoading).toBe(true);
    workspace = 'b';
    application.rerender(<Application />);
    await act(async () => requests.get('b')?.(saved('b', '#0f0')));
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('#00ff00');
    await act(async () => requests.get('a')?.(saved('a', '#f00')));
    expect(context.workspaceTheme?.workspace_id).toBe('b');
    authenticated = false;
    application.rerender(<Application />);
    expect(context.workspaceTheme).toBeNull();
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('');
  });

  it('does not overwrite a saved theme with an earlier load', async () => {
    render(<Application />);
    act(() => context.setWorkspaceTheme(saved('a', '#00f')));
    await act(async () => requests.get('a')?.(saved('a', '#f00')));
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('#0000ff');
    expect(context.workspaceThemeLoading).toBe(false);
  });

  it('clears previews and saved overrides when switching workspaces', async () => {
    const application = render(<Application />);
    await act(async () => requests.get('a')?.(saved('a', '#f00')));
    act(() => context.previewWorkspaceTheme(saved('a', '#00f')));
    workspace = 'b';
    application.rerender(<Application />);
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('');
    await act(async () => requests.get('b')?.(null));
    expect(context.workspaceTheme).toBeNull();
    expect(context.workspaceThemeLoading).toBe(false);
  });
  it('discards a response that arrives after logout', async () => {
    const application = render(<Application />);
    authenticated = false;
    application.rerender(<Application />);
    await act(async () => requests.get('a')?.(saved('a', '#f00')));
    expect(context.workspaceTheme).toBeNull();
    expect(context.workspaceThemeLoading).toBe(false);
    expect(document.documentElement.style.getPropertyValue('--ui-accent')).toBe('');
  });
});
