import type { ReactElement } from 'react';

const SKELETON_ROWS = [1, 2, 3];

export function SourceSkeleton(): ReactElement {
  return (
    <div className="sources-table-wrapper" aria-hidden="true">
      <table className="sources-table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Type</th>
            <th>Status</th>
            <th>URL</th>
            <th>Verified</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {SKELETON_ROWS.map((row) => (
            <tr key={row} className="source-card skeleton-card">
              <td>
                <div className="skeleton skeleton-title" />
              </td>
              <td>
                <div className="skeleton skeleton-text short" />
              </td>
              <td>
                <div className="skeleton skeleton-badge" />
              </td>
              <td>
                <div className="skeleton skeleton-text" />
              </td>
              <td>
                <div className="skeleton skeleton-text short" />
              </td>
              <td>
                <div className="skeleton skeleton-badge" />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
