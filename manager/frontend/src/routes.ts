import { hasStoredSession } from './api/session';
import { loadFeatureStylesheet } from './shared/stylesheet';

type Loader = () => Promise<unknown>;

export const loaders = {
  agentSignIn: () => import('./features/settings/ai/AgentSignInPage'),
  authenticatedProviders: () => import('./shared/components/AuthenticatedProviders'),
  chats: () => import('./features/chats/pages/ChatsPage'),
  emailVerification: () => import('./features/auth/pages/EmailVerificationPage'),
  invitationAccept: () => import('./features/auth/pages/InvitationAcceptPage'),
  layout: () => import('./shared/components/Layout/Layout'),
  models: () => import('./features/models/pages/ModelsPage'),
  orgSettings: () => import('./features/settings/organization/pages/OrgSettingsPage'),
  projects: () => import('./features/projects/pages/ProjectsPage'),
  resetPassword: () => import('./features/auth/pages/ResetPasswordPage'),
  sessions: () => import('./features/auth/pages/SessionsPage'),
  setup: () => import('./features/models/pages/SetupPage'),
  setupGate: () => import('./features/models/components/SetupGate'),
  sources: () => import('./features/sources/pages/SourcesPage'),
  tasks: () => import('./features/tasks/pages/TasksPage'),
  unauthorized: () => import('./pages/UnauthorizedPage'),
  wiki: () => import('./features/knowledge/pages/WikiPage'),
  workspaceSettings: () => import('./features/settings/workspace/pages/WorkspaceSettingsPage'),
};

const SIGNED_OUT_ROUTES: Record<string, Loader[]> = {
  '/invitations': [loaders.invitationAccept],
  '/reset-password': [loaders.resetPassword],
  '/unauthorized': [loaders.unauthorized],
  '/verify': [loaders.emailVerification],
  '/verify-email': [loaders.emailVerification],
};

const SHELL = [loaders.authenticatedProviders, loaders.setupGate, loaders.layout];

const SIGNED_IN_ROUTES: Record<string, Loader[]> = {
  '/': [...SHELL, loaders.chats],
  '/agent-sign-in': [loaders.authenticatedProviders, loaders.agentSignIn],
  '/chats': [...SHELL, loaders.chats],
  '/models': [...SHELL, loaders.models],
  '/org-settings': [...SHELL, loaders.orgSettings],
  '/projects': [...SHELL, loaders.projects],
  '/sessions': [...SHELL, loaders.sessions],
  '/settings': [...SHELL, loaders.workspaceSettings],
  '/setup': [loaders.authenticatedProviders, loaders.setup],
  '/sources': [...SHELL, loaders.sources],
  '/tasks': [...SHELL, loaders.tasks],
  '/wiki': [...SHELL, loaders.wiki],
};

const modules = new Map<Loader, unknown>();

export function preloaded<Module>(load: () => Promise<Module>): Module | undefined {
  return modules.get(load) as Module | undefined;
}

export async function preload<Module>(load: () => Promise<Module>): Promise<Module> {
  const [module] = await Promise.all([load(), loadFeatureStylesheet()]);
  modules.set(load, module);
  return module;
}

function run(routes: Loader[]): Promise<unknown> {
  return Promise.allSettled(routes.map(preload));
}

export function preloadRoute(
  pathname: string,
  storage: Pick<Storage, 'getItem'> = localStorage
): Promise<unknown> {
  const path = pathname.replace(/\/+$/, '') || '/';
  const routes =
    SIGNED_OUT_ROUTES[path] ?? (hasStoredSession(storage) ? SIGNED_IN_ROUTES[path] : undefined);
  return run(routes ?? []);
}

export function preloadSignedInRoutes(
  storage: Pick<Storage, 'getItem'> = localStorage
): Promise<unknown> {
  return run(hasStoredSession(storage) ? [...new Set(Object.values(SIGNED_IN_ROUTES).flat())] : []);
}

let firstRoute: Promise<unknown> = Promise.resolve();

export function startFirstRoute(pathname: string): void {
  firstRoute = preloadRoute(pathname);
}

export function firstRouteLoaded(): Promise<unknown> {
  return firstRoute;
}
