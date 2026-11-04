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
