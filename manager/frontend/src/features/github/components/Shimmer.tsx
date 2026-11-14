import { cn } from '@zone/ui';
import './Shimmer.css';

export interface ShimmerProps {
  className?: string;
}

export function Shimmer({ className }: ShimmerProps) {
  return <span className={cn('source-shimmer', className)} />;
}
