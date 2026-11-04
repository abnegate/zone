import './styles/cascade';
import { boot } from './boot';
import { afterFirstContentfulPaint } from './shared/firstPaint';
import { restoreLastPath } from './shared/lastPath';
import { reloadOnStaleChunk } from './shared/staleChunk';

reloadOnStaleChunk();
restoreLastPath();
afterFirstContentfulPaint(() => void boot(window.location.pathname));
