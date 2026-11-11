import type { ReactElement } from 'react';

export const COLUMNS = ['Name', 'Type', 'Status', 'URL', 'Verified', 'Actions'] as const;

const SKELETON_ROWS = [1, 2, 3];

export function SourceSkeleton(): ReactElement {
  return (
    <div className="sources-table-wrapper" aria-hidden="true">
      <table className="sources-table">
        <thead>
          <tr>
            {COLUMNS.map((column) => (
              <th key={column}>{column}</th>
            ))}
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
