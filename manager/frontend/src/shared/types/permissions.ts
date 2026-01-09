// Permission constants matching backend
export const PERMISSIONS = {
  ORGANIZATIONS: {
    CREATE: 'organizations:create',
    READ: 'organizations:read',
    UPDATE: 'organizations:update',
    DELETE: 'organizations:delete',
  },
  WORKSPACES: {
    CREATE: 'workspaces:create',
    READ: 'workspaces:read',
    UPDATE: 'workspaces:update',
    DELETE: 'workspaces:delete',
  },
  PROJECTS: {
    CREATE: 'projects:create',
    READ: 'projects:read',
    UPDATE: 'projects:update',
    DELETE: 'projects:delete',
  },
  TASKS: {
    CREATE: 'tasks:create',
    READ: 'tasks:read',
    UPDATE: 'tasks:update',
    DELETE: 'tasks:delete',
  },
  CHATS: {
    CREATE: 'chats:create',
    READ: 'chats:read',
    UPDATE: 'chats:update',
    DELETE: 'chats:delete',
  },
  SOURCES: {
    CREATE: 'sources:create',
    READ: 'sources:read',
    UPDATE: 'sources:update',
    DELETE: 'sources:delete',
  },
  MODELS: {
    CREATE: 'models:create',
    READ: 'models:read',
    UPDATE: 'models:update',
    DELETE: 'models:delete',
  },
  WIKI: {
    CREATE: 'wiki:create',
    READ: 'wiki:read',
    UPDATE: 'wiki:update',
    DELETE: 'wiki:delete',
  },
  USERS: {
    CREATE: 'users:create',
    READ: 'users:read',
    UPDATE: 'users:update',
    DELETE: 'users:delete',
  },
} as const;
