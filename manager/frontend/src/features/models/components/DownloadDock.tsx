import { useLocation } from 'react-router-dom';
import { usePull } from '../hooks/usePull';
import PullJobs from './PullJobs';
import './DownloadDock.css';

export default function DownloadDock() {
  const { jobs, activeCount, minimized, setMinimized, cancel, dismiss } = usePull();
  const { pathname } = useLocation();

  if (pathname === '/models' || jobs.length === 0) return null;

  if (minimized) {
    const label =
      activeCount > 0
        ? `${activeCount} download${activeCount === 1 ? '' : 's'} in progress`
        : 'Downloads finished';
    return (
      <button
        type="button"
        className="download-dock download-dock--minimized"
        onClick={() => setMinimized(false)}
        aria-label={`Expand downloads. ${label}`}
      >
        <span className="download-dock-dot" data-active={activeCount > 0} />
        {label}
      </button>
    );
  }

  return (
    <aside className="download-dock" aria-label="Model downloads">
      <header className="download-dock-header">
        <h2>Downloads</h2>
        <button type="button" className="pull-job-action" onClick={() => setMinimized(true)}>
          Minimize
        </button>
      </header>
      <PullJobs jobs={jobs} onCancel={cancel} onDismiss={dismiss} />
    </aside>
  );
}
