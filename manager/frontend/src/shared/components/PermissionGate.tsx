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

  // Check single permission
  if (permission) {
    return hasPermission(permission) ? <>{children}</> : <>{fallback}</>;
  }

  // Check multiple permissions
  if (permissions && permissions.length > 0) {
    const hasAccess = requireAll ? hasAllPermissions(permissions) : hasAnyPermission(permissions);
    return hasAccess ? <>{children}</> : <>{fallback}</>;
  }

  // No permissions specified, render children
  return <>{children}</>;
}
