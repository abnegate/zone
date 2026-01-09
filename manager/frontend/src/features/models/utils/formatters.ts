/**
 * Format a number with K/M suffix for thousands/millions
 * @param num - The number to format
 * @returns Formatted string with suffix
 */
export function formatNumber(num: number): string {
  if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
  if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
  return num.toString();
}
