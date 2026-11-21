import { scopeSegments } from './scope';
import type { AuditScope } from './types';

export const auditQueryKeys = {
  all: ['audit'] as const,
  idle: ['audit', 'idle'] as const,
  scope: (scope: AuditScope) => ['audit', ...scopeSegments(scope)] as const,
  destinations: (scope: AuditScope) => ['audit', ...scopeSegments(scope), 'destinations'] as const,
  retention: (scope: AuditScope) => ['audit', ...scopeSegments(scope), 'retention'] as const,
};
