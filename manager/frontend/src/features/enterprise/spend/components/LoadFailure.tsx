import { Button } from '@zone/ui';
import { RETRY_LABEL } from '../labels';

interface LoadFailureProps {
  error: string;
  onRetry(): void;
}

export function LoadFailure({ error, onRetry }: LoadFailureProps) {
  return (
    <div className="alert alert-error alert-inline" role="alert">
      <span>{error}</span>
      <Button type="button" variant="ghost" size="sm" onClick={onRetry}>
        {RETRY_LABEL}
      </Button>
    </div>
  );
}
