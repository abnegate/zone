export function formatEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '';
  if (seconds < 45) return 'less than a minute left';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return minutes === 1 ? 'about 1 minute left' : `about ${minutes} minutes left`;
  }
  const hours = Math.round(seconds / 3600);
  if (hours === 1) return 'about 1 hour left';
  if (hours < 48) return `about ${hours} hours left`;
  return 'more than a day left';
}

export function trainPercent(step?: number | null, total?: number | null): number | null {
  if (total == null || total <= 0 || step == null || step < 0) return null;
  return Math.min(100, Math.max(0, Math.round((step / total) * 100)));
}

export function trainHeadline(status?: string, name?: string | null): string {
  const suffix = name?.trim() ? ` ${name.trim()}` : '';
  if (status === 'succeeded') return `Training finished${suffix}`;
  if (status === 'failed') return `Training failed${suffix}`;
  return `Training${suffix}`;
}
