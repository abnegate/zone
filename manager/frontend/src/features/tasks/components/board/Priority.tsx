export interface PriorityProps {
  value: number;
}

export function Priority({ value }: PriorityProps) {
  return (
    <span className="task-card-priority" role="img" aria-label={`Priority ${value}`}>
      P{value}
    </span>
  );
}
