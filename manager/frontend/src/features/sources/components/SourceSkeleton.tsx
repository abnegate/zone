import type { ReactElement } from 'react';

export const READ_COLUMNS = ['Name', 'Type', 'Status', 'URL', 'Verified'] as const;

export const COLUMNS = [...READ_COLUMNS, 'Actions'] as const;

type Column = (typeof COLUMNS)[number];

const SKELETON_CELLS: Record<Column, string> = {
  Name: 'skeleton skeleton-title',
  Type: 'skeleton skeleton-text short',
  Status: 'skeleton skeleton-badge',
  URL: 'skeleton skeleton-text',
  Verified: 'skeleton skeleton-text short',
  Actions: 'skeleton skeleton-badge',
};

const SKELETON_ROWS = [1, 2, 3];

export function SourceSkeleton({ columns }: { columns: readonly Column[] }): ReactElement {
  return (
    <div className="sources-table-wrapper" aria-hidden="true">
      <table className="sources-table">
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column}>{column}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {SKELETON_ROWS.map((row) => (
            <tr key={row} className="source-card skeleton-card">
              {columns.map((column) => (
                <td key={column}>
                  <div className={SKELETON_CELLS[column]} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
