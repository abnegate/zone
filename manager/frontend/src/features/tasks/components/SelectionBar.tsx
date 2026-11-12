import { Button } from '@zone/ui';
import './SelectionBar.css';

const LABEL = 'Selected tasks';

export function SelectionBar({
  count,
  onEdit,
  onClear,
}: {
  count: number;
  onEdit: () => void;
  onClear: () => void;
}) {
  return (
    <div role="toolbar" aria-label={LABEL} className="tasks-selection">
      <span className="tasks-selection-count" aria-live="polite">
        {count} selected
      </span>
      <div className="tasks-selection-actions">
        <Button size="sm" onClick={onEdit}>
          Edit
        </Button>
        <Button size="sm" variant="ghost" onClick={onClear}>
          Clear
        </Button>
      </div>
    </div>
  );
}
