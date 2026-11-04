import React, { lazy, Suspense, useEffect, useState } from 'react';
import type { createRoot } from 'react-dom/client';
import App from './App';
import { hasStoredSession } from './api/session';
import { firstRouteLoaded, preloadSignedInRoutes } from './routes';
import { persistCurrentPath } from './shared/lastPath';
import { setUpServiceWorker } from './shared/serviceWorker';
import { loadFeatureStylesheet, loadFonts } from './shared/stylesheet';
import { releaseShellOnRender, showStartupFailure } from './shell/release';

const persist = () => persistCurrentPath();
(window as Window & { __zonePersistCurrentPath?: () => void }).__zonePersistCurrentPath = persist;
window.addEventListener('pagehide', persist);
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') {
    persist();
  }
});

const AFTER_FIRST_PAINT_DELAY_MILLISECONDS = 200;

const Toaster = lazy(() => import('sonner').then(({ Toaster }) => ({ default: Toaster })));

function AfterFirstPaint() {
  const [painted, setPainted] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => {
      if (signedIn) {
        void loadFeatureStylesheet();
        void preloadSignedInRoutes();
      } else {
        loadFonts();
      }
      setPainted(true);
    }, AFTER_FIRST_PAINT_DELAY_MILLISECONDS);
    return () => clearTimeout(timer);
  }, []);
  return painted ? (
    <Suspense fallback={null}>
      <Toaster />
    </Suspense>
  ) : null;
}

const signedIn = hasStoredSession();
if (signedIn) {
  loadFonts();
}

export function start(create: typeof createRoot): void {
  const container = document.getElementById('root') as HTMLElement;
  const root = create(container, {
    onUncaughtError: (error) => {
      showStartupFailure();
      reportError(error);
    },
  });
  void firstRouteLoaded().then(() => {
    releaseShellOnRender(container);
    root.render(
      <React.StrictMode>
        <App />
        <AfterFirstPaint />
      </React.StrictMode>
    );
    if (import.meta.env.VITE_DISABLE_PWA !== '1') {
      setUpServiceWorker();
    }
  });
}
