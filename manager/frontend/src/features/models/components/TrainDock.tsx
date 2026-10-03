import { useTrain } from '../hooks/useTrain';
import { trainHeadline, trainPercent } from '../utils/trainProgress';
import TrainMeter from './TrainMeter';
import './TrainDock.css';

export default function TrainDock() {
  const { job, minimized, setMinimized, dismiss } = useTrain();
  if (!job) return null;

  const percent = trainPercent(job.step, job.total);
  const running = job.status === 'running';
  const title = trainHeadline(job.status, job.name);

  if (minimized) {
    const label =
      percent != null && running ? `${title} · ${percent}%` : running ? `${title}…` : title;
    return (
      <button
        type="button"
        className="train-dock train-dock--minimized"
        onClick={() => setMinimized(false)}
        aria-label={`Expand training. ${label}`}
      >
        <span className="train-dock-dot" data-active={running} />
        {label}
      </button>
    );
  }

  return (
    <aside className="train-dock" aria-label="LoRA training">
      <header className="train-dock-header">
        <h2>{title}</h2>
        {running ? (
          <button type="button" className="train-dock-action" onClick={() => setMinimized(true)}>
            Minimize
          </button>
        ) : (
          <button type="button" className="train-dock-action" onClick={dismiss}>
            Dismiss
          </button>
        )}
      </header>
      {job.status === 'failed' && job.error ? (
        <p className="train-dock-error">{job.error}</p>
      ) : (
        <TrainMeter
          job={job}
          detail={running ? 'This run keeps going if you leave the page.' : undefined}
        />
      )}
    </aside>
  );
}
