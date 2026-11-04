import React from 'react';
import ReactDOM from 'react-dom/client';
import { Toaster } from 'sonner';
import App from './App';
import { persistCurrentPath, restoreLastPath } from './shared/lastPath';
import { stylesheetLoaded } from './shared/stylesheet';

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

const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement);
void stylesheetLoaded().then(() => {
  root.render(
    <React.StrictMode>
      <App />
      <Toaster />
    </React.StrictMode>
  );
});
