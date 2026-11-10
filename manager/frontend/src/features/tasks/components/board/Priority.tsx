export interface PriorityProps {
  value: number;
}

export function Priority({ value }: PriorityProps) {
  const label = `Priority ${value}`;
  return (
    <span className="task-card-priority" role="img" aria-label={label} title={label}>
      P{value}
    </span>
  );
}
