import type { ReactNode } from 'react';
import { Navigate, useLocation } from 'react-router-dom';
import { useAuth } from '../../features/auth';
import type { OrgRole } from '../../types';
import { useWorkspace } from '../context/WorkspaceContext';

export type OrganizationRoleRequirement = Exclude<OrgRole, 'member'>;

const ORGANIZATION_RANK: Record<OrgRole, number> = { member: 0, admin: 1, owner: 2 };

export function meetsOrganizationRole(
  role: OrgRole | undefined,
  required: OrganizationRoleRequirement
): boolean {
  return role !== undefined && ORGANIZATION_RANK[role] >= ORGANIZATION_RANK[required];
}

function Loading() {
  return (
    <div className="loading-container">
      <span className="spinner" />
      <span>Loading...</span>
    </div>
  );
}

interface OrganizationRoleGateProps {
  children: ReactNode;
  required: OrganizationRoleRequirement;
  useWorkspaceHook?: typeof useWorkspace;
}

// Permissions are global to the account; the role is held per organization,
// so a member of one tenant does not reach another tenant's settings.
function OrganizationRoleGate({ children, required, useWorkspaceHook }: OrganizationRoleGateProps) {
  const { loading, currentOrganization } = (useWorkspaceHook ?? useWorkspace)();

  if (loading) {
    return <Loading />;
  }

  if (currentOrganization && !meetsOrganizationRole(currentOrganization.role, required)) {
    return <Navigate to="/unauthorized" replace />;
  }

  return <>{children}</>;
}

interface ProtectedRouteProps {
  children: ReactNode;
  requiredPermission?: string;
  requiredPermissions?: string[];
  requireAll?: boolean;
  requiredOrganizationRole?: OrganizationRoleRequirement;
  useAuthHook?: typeof useAuth;
  useWorkspaceHook?: typeof useWorkspace;
}

export default function ProtectedRoute({
  children,
  requiredPermission,
  requiredPermissions,
  requireAll = false,
  requiredOrganizationRole,
  useAuthHook,
  useWorkspaceHook,
}: ProtectedRouteProps) {
  const auth = (useAuthHook ?? useAuth)();
  const { isAuthenticated, isLoading, hasPermission, hasAnyPermission, hasAllPermissions } = auth;
  const location = useLocation();

  if (isLoading) {
    return <Loading />;
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" state={{ from: location }} replace />;
  }

  if (requiredPermission && !hasPermission(requiredPermission)) {
    return <Navigate to="/unauthorized" replace />;
  }

  if (requiredPermissions && requiredPermissions.length > 0) {
    const hasAccess = requireAll
      ? hasAllPermissions(requiredPermissions)
      : hasAnyPermission(requiredPermissions);

    if (!hasAccess) {
      return <Navigate to="/unauthorized" replace />;
    }
  }

  if (requiredOrganizationRole) {
    return (
      <OrganizationRoleGate required={requiredOrganizationRole} useWorkspaceHook={useWorkspaceHook}>
        {children}
      </OrganizationRoleGate>
    );
  }

  return <>{children}</>;
}
