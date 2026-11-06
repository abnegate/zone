import { type ReactElement, useEffect, useState } from 'react';

export const RESPONDING = 'Responding';

export function Generation({ status }: { status: string | null }): ReactElement {
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    const started = performance.now();
    const interval = window.setInterval(() => {
      setElapsed(Math.floor((performance.now() - started) / 1000));
    }, 1000);
    return () => window.clearInterval(interval);
  }, []);

  return (
    <div className="message-status">
      <span className="generation-spinner" aria-hidden="true" />
      <span role="status" className={status ? undefined : 'sr-only'}>
        {status || RESPONDING}
      </span>
      <span className="generation-elapsed" role="timer" aria-label="Time elapsed">
        {Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, '0')}
      </span>
    </div>
  );
}
