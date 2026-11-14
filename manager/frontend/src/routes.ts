import { hasStoredSession } from './api/session';
import { loadFeatureStylesheet } from './shared/stylesheet';

type Loader = () => Promise<unknown>;

export const loaders = {
  agentSignIn: () => import('./features/settings/ai/AgentSignInPage'),
  authenticatedProviders: () => import('./shared/components/AuthenticatedProviders'),
  chats: () => import('./features/chats/pages/ChatsPage'),
  layout: () => import('./shared/components/Layout/Layout'),
  models: () => import('./features/models/pages/ModelsPage'),
  projects: () => import('./features/projects/pages/ProjectsPage'),
  pullRequests: () => import('./features/pull-requests/pages/PullRequestsPage'),
  releases: () => import('./features/releases/pages/ReleasesPage'),
  settings: () => import('./features/settings/pages/SettingsPage'),
  setup: () => import('./features/models/pages/SetupPage'),
  setupGate: () => import('./features/models/components/SetupGate'),
  tasks: () => import('./features/tasks/pages/TasksPage'),
  wiki: () => import('./features/knowledge/pages/WikiPage'),
};

const SHELL = [loaders.authenticatedProviders, loaders.setupGate, loaders.layout];

const SIGNED_IN_ROUTES: Record<string, Loader[]> = {
  '/': [...SHELL, loaders.chats],
  '/agent-sign-in': [loaders.authenticatedProviders, loaders.agentSignIn],
  '/chats': [...SHELL, loaders.chats],
  '/models': [...SHELL, loaders.models],
  '/org-settings': [...SHELL, loaders.settings],
  '/projects': [...SHELL, loaders.projects],
  '/pull-requests': [...SHELL, loaders.pullRequests],
  '/releases': [...SHELL, loaders.releases],
  '/sessions': [...SHELL, loaders.settings],
  '/settings': [...SHELL, loaders.settings],
  '/setup': [loaders.authenticatedProviders, loaders.setup],
  '/sources': [...SHELL, loaders.settings],
  '/tasks': [...SHELL, loaders.tasks],
  '/wiki': [...SHELL, loaders.wiki],
};

const NESTED_ROUTES = new Set(['/settings']);

function routeKey(pathname: string): string {
  const path = pathname.replace(/\/+$/, '') || '/';
  const root = path.match(/^\/[^/]*/)?.[0] ?? path;
  return NESTED_ROUTES.has(root) ? root : path;
}

const modules = new Map<Loader, unknown>();

export function preloaded<Module>(load: () => Promise<Module>): Module | undefined {
  return modules.get(load) as Module | undefined;
}

export async function preload<Module>(load: () => Promise<Module>): Promise<Module> {
  const stylesheet = loadFeatureStylesheet();
  const [module] = await Promise.all([load(), stylesheet]);
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
  return run((hasStoredSession(storage) && SIGNED_IN_ROUTES[routeKey(pathname)]) || []);
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
