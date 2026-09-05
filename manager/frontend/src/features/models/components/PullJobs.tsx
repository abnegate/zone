import type { PullJob } from '../types';
import { formatBytes } from '../utils';
import './PullJobs.css';

interface PullJobsProps {
  jobs: PullJob[];
  onCancel: (id: string) => void;
  onDismiss: (id: string) => void;
}

export default function PullJobs({ jobs, onCancel, onDismiss }: PullJobsProps) {
  if (jobs.length === 0) return null;

  return (
    <div className="pull-jobs" aria-live="polite">
      {jobs.map((job) => (
        <article key={job.id} className="progress-section pull-job">
          <div className="progress-header">
            <div className="pull-job-title">
              <span>
                {job.pulling
                  ? 'Installing model...'
                  : job.result?.success
                    ? 'Installation complete'
                    : 'Installation failed'}
              </span>
              {job.modelName && <span className="pull-job-name">{job.modelName}</span>}
            </div>
            {job.pulling ? (
              <button type="button" className="pull-job-action" onClick={() => onCancel(job.id)}>
                Cancel
              </button>
            ) : (
              <button type="button" className="pull-job-action" onClick={() => onDismiss(job.id)}>
                Dismiss
              </button>
            )}
          </div>

          {job.progress !== null && (
            <div className="progress-bar-container">
              <div className="progress-bar" style={{ width: `${job.progress}%` }} />
              <span className="progress-text">{Math.round(job.progress)}%</span>
            </div>
          )}

          {job.chunk && (
            <p className="progress-chunk">
              {formatBytes(job.chunk.completed)} / {formatBytes(job.chunk.total)}
              {job.chunk.digest
                ? ` · ${job.chunk.digest.replace(/^sha256:/, '').slice(0, 12)}`
                : ''}
            </p>
          )}

          {job.steps.length > 0 && (
            <div className="steps-list">
              {job.steps.map((step) => (
                <div
                  key={`${job.id}-${step.name}-${step.status}`}
                  className={`step-item step-${step.status}`}
                >
                  <span className="step-icon">
                    {step.status === 'success' ? '✓' : step.status === 'error' ? '✗' : '○'}
                  </span>
                  <span className="step-name">{step.name}</span>
                  <span className="step-message">{step.message}</span>
                </div>
              ))}
            </div>
          )}

          {job.result && (
            <div
              className={`result-message ${job.result.success ? 'result-success' : 'result-error'}`}
            >
              {job.result.message}
            </div>
          )}
        </article>
      ))}
    </div>
  );
}
