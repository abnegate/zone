import { type ReactElement, useEffect, useState } from 'react';

export const RESPONDING = 'Responding';

/** Whole seconds a turn begun at `started` has been running, never negative. */
function running(started: number): number {
  return Math.max(0, Math.floor((Date.now() - started) / 1000));
}

export function Generation({
  status,
  started,
}: {
  status: string | null;
  /** When the running turn began, or null when nothing remembers it. */
  started: number | null;
}): ReactElement {
  // A turn whose start nothing remembered is counted from here, as it was
  // before the clock was kept, rather than not counted at all.
  const [mounted] = useState(() => Date.now());
  const begun = started ?? mounted;
  const [elapsed, setElapsed] = useState(() => running(begun));

  // Counted from when the turn started rather than from when this mounted:
  // swapping to another chat and back unmounts it while the turn goes on, and
  // a count that restarted there would report seconds of a minutes-long wait.
  useEffect(() => {
    setElapsed(running(begun));
    const interval = window.setInterval(() => setElapsed(running(begun)), 1000);
    return () => window.clearInterval(interval);
  }, [begun]);

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
