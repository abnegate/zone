import type { TrainJob } from '../../../api/models';
import { formatEta, trainHeadline, trainPercent } from '../utils/trainProgress';
import './TrainMeter.css';

export default function TrainMeter({ job, detail }: { job: TrainJob; detail?: string }) {
  const percent = trainPercent(job.step, job.total);
  const eta = job.status === 'running' && job.eta_seconds != null ? formatEta(job.eta_seconds) : '';
  const stepLabel =
    job.total != null && job.step != null
      ? job.step === 0
        ? 'Starting training'
        : `Step ${job.step} of ${job.total}`
      : job.status === 'running'
        ? 'Preparing the dataset'
        : null;

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
        aria-label={trainHeadline(job.status, job.name)}
      >
        <div
          className={
            percent == null ? 'progress-bar-fill progress-bar-fill--unknown' : 'progress-bar-fill'
          }
          style={percent == null ? undefined : { width: `${percent}%` }}
        />
      </div>
      {(eta || detail) && (
        <p className="train-meter-eta">
          {eta}
          {eta && detail ? ' · ' : ''}
          {detail}
        </p>
      )}
    </div>
  );
}
