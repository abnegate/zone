import './styles/cascade';
import { reloadOnStaleChunk } from './shared/staleChunk';

function boot() {
  void import('./main');
}

reloadOnStaleChunk();

if (document.hidden) {
  boot();
} else {
  requestAnimationFrame(() => requestAnimationFrame(boot));
}
