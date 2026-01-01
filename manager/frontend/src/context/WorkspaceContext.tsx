import { type ReactNode, createContext, useCallback, useContext, useEffect, useState } from 'react';
import { client } from '../api/client';
import type { Organization, Workspace } from '../types';

interface WorkspaceContextType {
  organizations: Organization[];
  currentOrganization: Organization | null;
  currentWorkspace: Workspace | null;
  workspaces: Workspace[];
  loading: boolean;
  error: string | null;
  setCurrentOrganization: (org: Organization) => void;
  setCurrentWorkspace: (ws: Workspace) => void;
  refreshOrganizations: () => Promise<void>;
  refreshWorkspaces: () => Promise<void>;
}

const WorkspaceContext = createContext<WorkspaceContextType | undefined>(undefined);

const ORG_STORAGE_KEY = 'manager_current_org';
const WS_STORAGE_KEY = 'manager_current_workspace';

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [organizations, setOrganizations] = useState<Organization[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [currentOrganization, setCurrentOrgState] = useState<Organization | null>(null);
  const [currentWorkspace, setCurrentWsState] = useState<Workspace | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refreshOrganizations = useCallback(async () => {
    try {
      setError(null);
      const orgs = await client.getOrganizations(true);
      setOrganizations(orgs);

      // Restore from localStorage or pick first
      const savedOrgId = localStorage.getItem(ORG_STORAGE_KEY);
      const savedOrg = orgs.find((o) => o.id === savedOrgId) || orgs[0] || null;
      if (savedOrg) {
        setCurrentOrgState(savedOrg);
        localStorage.setItem(ORG_STORAGE_KEY, savedOrg.id);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load organizations');
    }
  }, []);

  const refreshWorkspaces = useCallback(async () => {
    if (!currentOrganization) {
      setWorkspaces([]);
      setCurrentWsState(null);
      return;
    }

    try {
      setError(null);
      const wsList = await client.getWorkspaces(currentOrganization.id, true);
      setWorkspaces(wsList);

      // Restore from localStorage or pick first
      const savedWsId = localStorage.getItem(WS_STORAGE_KEY);
      const savedWs = wsList.find((w) => w.id === savedWsId) || wsList[0] || null;
      if (savedWs) {
        setCurrentWsState(savedWs);
        localStorage.setItem(WS_STORAGE_KEY, savedWs.id);
      } else {
        setCurrentWsState(null);
        localStorage.removeItem(WS_STORAGE_KEY);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load workspaces');
    }
  }, [currentOrganization]);

  const setCurrentOrganization = useCallback((org: Organization) => {
    setCurrentOrgState(org);
    localStorage.setItem(ORG_STORAGE_KEY, org.id);
    // Clear workspace selection when org changes
    setCurrentWsState(null);
    setWorkspaces([]);
    localStorage.removeItem(WS_STORAGE_KEY);
  }, []);

  const setCurrentWorkspace = useCallback((ws: Workspace) => {
    setCurrentWsState(ws);
    localStorage.setItem(WS_STORAGE_KEY, ws.id);
  }, []);

  // Load organizations on mount
  useEffect(() => {
    refreshOrganizations().finally(() => setLoading(false));
  }, [refreshOrganizations]);

  // Load workspaces when organization changes
  useEffect(() => {
    if (currentOrganization) {
      refreshWorkspaces();
    }
  }, [currentOrganization, refreshWorkspaces]);

  return (
    <WorkspaceContext.Provider
      value={{
        organizations,
        currentOrganization,
        currentWorkspace,
        workspaces,
        loading,
        error,
        setCurrentOrganization,
        setCurrentWorkspace,
        refreshOrganizations,
        refreshWorkspaces,
      }}
    >
      {children}
    </WorkspaceContext.Provider>
  );
}

export function useWorkspace() {
  const context = useContext(WorkspaceContext);
  if (context === undefined) {
    throw new Error('useWorkspace must be used within a WorkspaceProvider');
  }
  return context;
}
