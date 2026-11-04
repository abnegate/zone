export const TIME_ZONE_HEADER = 'X-Zone-Time-Zone';

export function timeZoneHeaders(): Record<string, string> {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    return zone ? { [TIME_ZONE_HEADER]: zone } : {};
  } catch {
    return {};
  }
}
