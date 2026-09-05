import { Button } from '@zone/ui';
import { Link, useLocation } from 'react-router-dom';
import { usePull } from '../hooks/usePull';
import { formatBytes } from '../utils';
import './PullDownloadIndicator.css';

export default function PullDownloadIndicator() {
  const { pulling, progress, chunk, model, cancel } = usePull();
  const location = useLocation();

  if (!pulling || location.pathname === '/models') {
    return null;
  }

  return (
    <div className="pull-download-indicator" role="status" aria-live="polite">
      <div className="pull-download-indicator-copy">
        <Link to="/models" className="pull-download-indicator-title">
          Downloading {model || 'model'}
        </Link>
        <p className="pull-download-indicator-meta">
          {progress !== null ? `${Math.round(progress)}%` : 'Starting…'}
          {chunk
            ? ` · ${formatBytes(chunk.completed)} / ${formatBytes(chunk.total)}`
            : ' · continues in the background'}
        </p>
      </div>
      {progress !== null && (
        <div className="pull-download-indicator-bar" aria-hidden="true">
          <span style={{ width: `${progress}%` }} />
        </div>
      )}
      <Button type="button" variant="ghost" size="sm" onClick={cancel}>
        Cancel
      </Button>
    </div>
  );
}
