import type { TrainJob } from '../../../api/models';
import {
  formatEta,
  formatLoss,
  trainHeadline,
  trainJobPercent,
  trainStepLabel,
} from '../utils/trainProgress';
import './TrainMeter.css';

export default function TrainMeter({ job, detail }: { job: TrainJob; detail?: string }) {
  const percent = trainJobPercent(job);
  const eta = job.status === 'running' && job.eta_seconds != null ? formatEta(job.eta_seconds) : '';
  const loss = job.status === 'running' ? formatLoss(job.loss) : '';
  const stepLabel = trainStepLabel(job);
  const footnote = [eta, loss, detail].filter(Boolean).join(' · ');

  return (
    <div className="train-meter">
      <div className="train-meter-copy">
        {stepLabel && <span>{stepLabel}</span>}
        {percent != null && <span className="train-meter-percent">{percent}%</span>}
      </div>
      <div
        className="progress-bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
        aria-label={trainHeadline(job.status, job.name, job.method)}
      >
        <div
          className={
            percent == null ? 'progress-bar-fill progress-bar-fill--unknown' : 'progress-bar-fill'
          }
          style={percent == null ? undefined : { width: `${percent}%` }}
        />
      </div>
      {footnote && <p className="train-meter-eta">{footnote}</p>}
    </div>
  );
}
