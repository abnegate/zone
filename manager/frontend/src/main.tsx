import React, { lazy, Suspense, useEffect, useState } from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { persistCurrentPath, restoreLastPath } from './shared/lastPath';
import { loadFeatureStylesheet, loadFonts, stylesheetsLoaded } from './shared/stylesheet';

restoreLastPath();
const persist = () => persistCurrentPath();
(window as Window & { __zonePersistCurrentPath?: () => void }).__zonePersistCurrentPath = persist;
window.addEventListener('pagehide', persist);
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') {
    persist();
  }
});

if (
  'serviceWorker' in navigator &&
  (window.location.hostname === '127.0.0.1' || window.location.hostname === 'localhost')
) {
  void navigator.serviceWorker.getRegistrations().then((registrations) => {
    for (const registration of registrations) {
      void registration.unregister();
    }
  });
}

const AFTER_FIRST_PAINT_DELAY_MILLISECONDS = 200;

const Toaster = lazy(() => import('sonner').then(({ Toaster }) => ({ default: Toaster })));

function AfterFirstPaint() {
  const [painted, setPainted] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => {
      loadFonts();
      void loadFeatureStylesheet();
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

const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement);
void stylesheetsLoaded().then(() => {
  root.render(
    <React.StrictMode>
      <App />
      <AfterFirstPaint />
    </React.StrictMode>
  );
});
