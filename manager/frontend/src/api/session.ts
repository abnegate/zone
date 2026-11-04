export const ACCESS_TOKEN_KEY = 'manager_access_token';
export const REFRESH_TOKEN_KEY = 'manager_refresh_token';
export const ACTIVE_PULLS_KEY = 'zone.activePulls';
export const LEGACY_ACTIVE_PULL_KEY = 'zone.activePull';

type EnsureAccessToken = () => Promise<string | null>;

let accessToken: string | null = null;
let ensure: EnsureAccessToken | null = null;

export const session = {
  setAccessToken(token: string | null): void {
    accessToken = token;
  },
  getAccessToken(): string | null {
    return accessToken;
  },
  setEnsureAccessToken(next: EnsureAccessToken | null): void {
    ensure = next;
  },
  ensureAccessToken(): Promise<string | null> {
    return ensure ? ensure() : Promise.resolve(accessToken);
  },
};

export function hasStoredSession(storage: Pick<Storage, 'getItem'> = localStorage): boolean {
  return Boolean(storage.getItem(ACCESS_TOKEN_KEY) || storage.getItem(REFRESH_TOKEN_KEY));
}
