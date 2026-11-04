import './styles/cascade';
import { startFirstRoute } from './routes';
import { restoreLastPath } from './shared/lastPath';
import { reloadOnStaleChunk } from './shared/staleChunk';

function boot() {
  startFirstRoute(window.location.pathname);
  void import('./main');
}

reloadOnStaleChunk();
restoreLastPath();

if (document.hidden) {
  boot();
} else {
  requestAnimationFrame(() => requestAnimationFrame(boot));
}
