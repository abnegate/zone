import type { ReactNode } from 'react';
import { useAuth } from '../../features/auth';

interface PermissionGateProps {
  children: ReactNode;
  permission?: string;
  permissions?: string[];
  requireAll?: boolean;
  fallback?: ReactNode;
  useAuthHook?: typeof useAuth;
}

export default function PermissionGate({
  children,
  permission,
  permissions,
  requireAll = false,
  fallback = null,
  useAuthHook,
}: PermissionGateProps) {
  const auth = (useAuthHook ?? useAuth)();
  const { hasPermission, hasAnyPermission, hasAllPermissions } = auth;

  if (permission) {
    return hasPermission(permission) ? children : fallback;
  }

  if (permissions && permissions.length > 0) {
    const hasAccess = requireAll ? hasAllPermissions(permissions) : hasAnyPermission(permissions);
    return hasAccess ? children : fallback;
  }

  return children;
}
