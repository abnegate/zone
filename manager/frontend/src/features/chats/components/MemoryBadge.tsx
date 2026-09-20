import { Badge } from '@zone/ui';

const LABEL = 'Memory read';
const TITLE = 'The assistant read your stored memory while writing this reply.';

/// The one thing the server can vouch for about memory on a reply: the model
/// read it. Whether the reading changed the answer is the model's claim, so the
/// chip says "read" and wears the neutral badge rather than a warning colour.
export function MemoryBadge({ used }: { used?: boolean }) {
  if (!used) return null;

  return (
    <Badge variant="neutral" className="memory-badge" data-testid="memory-badge" title={TITLE}>
      {LABEL}
    </Badge>
  );
}
