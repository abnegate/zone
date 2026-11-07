export const SERVICE_WORKER_FILE = 'sw.js';
export const SERVICE_WORKER_IDLE_TIMEOUT_MILLISECONDS = 5000;
export const SERVICE_WORKER_NAVIGATION_DENYLIST: RegExp[] = [/^\/api\//];

const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1']);

type Host = Pick<Window, 'addEventListener' | 'setTimeout'> & {
  document: Pick<Document, 'readyState'>;
  location: Pick<Location, 'hostname'>;
  navigator: { serviceWorker?: Pick<ServiceWorkerContainer, 'register' | 'getRegistrations'> };
  requestIdleCallback?: Window['requestIdleCallback'];
};

function afterLoadWhenIdle(host: Host, callback: () => void): void {
  const idle = () => {
    if (typeof host.requestIdleCallback === 'function') {
      host.requestIdleCallback(callback, { timeout: SERVICE_WORKER_IDLE_TIMEOUT_MILLISECONDS });
    } else {
      host.setTimeout(callback, 0);
    }
  };
  if (host.document.readyState === 'complete') {
    idle();
  } else {
    host.addEventListener('load', idle, { once: true });
  }
}

export function setUpServiceWorker(host: Host = window): void {
  const workers = host.navigator.serviceWorker;
  if (!workers) {
    return;
  }
  if (LOCAL_HOSTS.has(host.location.hostname)) {
    void workers.getRegistrations().then((registrations) => {
      for (const registration of registrations) {
        void registration.unregister();
      }
    });
    return;
  }
  afterLoadWhenIdle(host, () => {
    void workers.register(`/${SERVICE_WORKER_FILE}`, { scope: '/' });
  });
}
