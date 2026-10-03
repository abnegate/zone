import { useTrain } from '../hooks/useTrain';
import { trainHeadline, trainPercent } from '../utils/trainProgress';
import TrainMeter from './TrainMeter';
import './TrainDock.css';

export default function TrainDock() {
  const { job, dismiss } = useTrain();
  if (!job) return null;

  const percent = trainPercent(job.step, job.total);
  const running = job.status === 'running';
  const title = trainHeadline(job.status, job.name);

  return (
    <aside className="train-dock" aria-label="LoRA training" title={title}>
      <header className="train-dock-header">
        <h2>{title}</h2>
        {!running && (
          <button type="button" className="train-dock-action" onClick={dismiss}>
            Dismiss
          </button>
        )}
      </header>
      <span className="train-dock-collapsed-percent">
        {percent != null ? `${percent}%` : running ? '…' : '!'}
      </span>
      {job.status === 'failed' && job.error ? (
        <p className="train-dock-error">{job.error}</p>
      ) : (
        <TrainMeter job={job} />
      )}
    </aside>
  );
}
