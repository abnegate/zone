import { useEffect, useRef } from 'react';

interface StatusLine {
  message: string;
  type?: 'normal' | 'success' | 'error' | 'retry' | 'in-progress';
  id?: string;
}

interface StatusLogProps {
  lines: StatusLine[];
}

export function StatusLog({ lines }: StatusLogProps) {
  const logRef = useRef<HTMLDivElement>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: lines triggers auto-scroll when new logs arrive
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [lines]);

  return (
    <div
      className="max-h-52 space-y-2 overflow-auto rounded-md border bg-muted/30 p-3 text-xs font-mono text-muted-foreground"
      ref={logRef}
      data-testid="status-log"
    >
      {lines.map((line, index) => (
        // Prefer ids for stable updates, fallback to index + message for append-only logs.
        <div
          key={line.id ?? `${index}-${line.message.slice(0, 20)}`}
          className="flex gap-2"
          data-status-line
          data-status={line.type || 'normal'}
        >
          {line.type === 'in-progress' ? (
            <span
              className="mt-0.5 inline-flex h-3 w-3 animate-spin rounded-full border border-muted-foreground border-t-transparent"
              aria-hidden="true"
            />
          ) : (
            <span
              className={`${
                line.type === 'success'
                  ? 'text-emerald-600'
                  : line.type === 'error'
                    ? 'text-destructive'
                    : line.type === 'retry'
                      ? 'text-amber-600'
                      : 'text-muted-foreground'
              }`}
            >
              {line.type === 'success'
                ? '✓'
                : line.type === 'error'
                  ? '✗'
                  : line.type === 'retry'
                    ? '↻'
                    : '›'}
            </span>
          )}
          <span>{line.message}</span>
        </div>
      ))}
    </div>
  );
}
