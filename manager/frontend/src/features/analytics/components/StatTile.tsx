import { Card } from '@zone/ui';
import type { ReactNode } from 'react';

export interface StatTileProps {
  label: string;
  value: string;
  detail?: ReactNode;
}

export function StatTile({ label, value, detail }: StatTileProps) {
  return (
    <Card data-testid="analytics-tile" className="analytics-tile">
      <span className="analytics-tile-label">{label}</span>
      <span className="analytics-tile-value">{value}</span>
      {detail && <span className="analytics-tile-detail">{detail}</span>}
    </Card>
  );
}
