import './MemoryBadge.css';

const LABEL = 'Memory read';
const TITLE = 'The assistant read your stored memory while writing this reply.';

export function MemoryBadge({ used }: { used?: boolean }) {
  if (!used) return null;

  return (
    <span className="memory-badge" data-testid="memory-badge" title={TITLE}>
      {LABEL}
    </span>
  );
}
