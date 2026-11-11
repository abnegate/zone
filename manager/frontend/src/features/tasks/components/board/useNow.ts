import { useEffect, useState } from 'react';

const MINUTE_MS = 60_000;

export function useNow(paused: boolean): number {
  const [now, setNow] = useState(Date.now);

  useEffect(() => {
    if (paused) return undefined;
    const timer = setInterval(() => setNow(Date.now()), MINUTE_MS);
    return () => clearInterval(timer);
  }, [paused]);

  return now;
}
