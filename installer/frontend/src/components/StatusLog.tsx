import { useEffect, useRef } from 'react';

interface StatusLine {
  message: string;
  type?: 'normal' | 'success' | 'error' | 'retry';
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
    <div className="status-log" ref={logRef}>
      {lines.map((line, index) => (
        // Using index + message as key since log lines are append-only and have no unique IDs
        <div
          key={`${index}-${line.message.slice(0, 20)}`}
          className={`status-line ${line.type || ''}`}
        >
          {line.message}
        </div>
      ))}
    </div>
  );
}
