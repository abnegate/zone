import { useEffect, useState } from 'react';

const LONGEST_TIMER = 2 ** 31 - 1;

export function useExpired(at: string | null): boolean {
  const deadline = at === null ? Number.NaN : Date.parse(at);
  const [checked, setChecked] = useState(Date.now);

  useEffect(() => {
    if (Number.isNaN(deadline) || checked >= deadline) return;
    const remaining = Math.min(Math.max(deadline - Date.now(), 0), LONGEST_TIMER);
    const timer = setTimeout(() => setChecked(Date.now()), remaining);
    return () => clearTimeout(timer);
  }, [deadline, checked]);

  return deadline <= checked;
}
