import { AuditScopeKind } from './schemas';
import type { AuditScope } from './types';

export const SERVER_SCOPE: AuditScope = { kind: AuditScopeKind.Server };

export function organizationScope(organizationId: string): AuditScope {
  return { kind: AuditScopeKind.Organization, organizationId };
}

export function scopeSegments(scope: AuditScope): readonly string[] {
  return scope.kind === AuditScopeKind.Organization
    ? [scope.kind, scope.organizationId]
    : [scope.kind];
}
