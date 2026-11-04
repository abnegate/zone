import './styles/cascade';
import { startFirstRoute } from './routes';
import { restoreLastPath } from './shared/lastPath';
import { reloadOnStaleChunk } from './shared/staleChunk';
import { releaseShell } from './shell/release';

function boot() {
  startFirstRoute(window.location.pathname);
  import('./main').catch(() => {
    releaseShell();
    const text = document.querySelector('.loader-text');
    if (text) {
      text.textContent = 'Zone could not load. Reload the page to try again.';
    }
  });
}

reloadOnStaleChunk();
restoreLastPath();

if (document.hidden) {
  boot();
} else {
  requestAnimationFrame(() => requestAnimationFrame(boot));
}
