import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import '@zone/ui/styles';
import * as serviceWorkerRegistration from './serviceWorkerRegistration';

const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement);
root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);

// Register service worker for PWA functionality
serviceWorkerRegistration.register();
