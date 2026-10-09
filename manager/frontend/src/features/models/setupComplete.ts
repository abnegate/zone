export function setupCompleteKey(userId: string): string {
  return `manager_setup_complete:${userId}`;
}

export function isSetupComplete(userId: string): boolean {
  try {
    return localStorage.getItem(setupCompleteKey(userId)) === '1';
  } catch {
    return false;
  }
}

export function markSetupComplete(userId: string): void {
  try {
    localStorage.setItem(setupCompleteKey(userId), '1');
  } catch {
    // Private browsing can reject localStorage writes.
  }
}
