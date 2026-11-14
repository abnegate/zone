import { Shimmer } from '../../github';

const PLACEHOLDERS = ['first', 'second', 'third'] as const;

export const LOADING = 'Loading releases';

export function ReleasesSkeleton() {
  return (
    <>
      <p className="sr-only" role="status">
        {LOADING}
      </p>
      <div className="release-day releases-skeleton" aria-hidden="true">
        <div className="release-day-header">
          <Shimmer className="release-skeleton-label" />
        </div>
        <div className="release-day-list">
          {PLACEHOLDERS.map((placeholder) => (
            <div key={placeholder} className="release-card card">
              <div className="release-card-header">
                <div className="release-card-title">
                  <Shimmer className="release-skeleton-title" />
                </div>
                <div className="release-card-meta">
                  <Shimmer className="release-skeleton-meta" />
                </div>
                <div className="release-card-footer">
                  <Shimmer className="release-skeleton-footer" />
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
