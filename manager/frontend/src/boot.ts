import type { createRoot } from 'react-dom/client';
import { startFirstRoute } from './routes';
import { showStartupFailure } from './shell/release';

export interface BootSteps {
  route: (pathname: string) => void;
  renderer: () => Promise<{ createRoot: typeof createRoot }>;
  app: () => Promise<{ start: (create: typeof createRoot) => void }>;
  fail: () => void;
}

const steps: BootSteps = {
  route: startFirstRoute,
  renderer: () => import('./renderer'),
  app: () => import('./main'),
  fail: () => showStartupFailure(),
};

export async function boot(pathname: string, using: BootSteps = steps): Promise<void> {
  try {
    using.route(pathname);
    const [{ createRoot }, { start }] = await Promise.all([using.renderer(), using.app()]);
    start(createRoot);
  } catch {
    using.fail();
  }
}
