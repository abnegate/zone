import { deviceHeaders } from './device';
import { session } from './session';

export abstract class AuthorizedApi {
  private getAccessToken: () => string | null = () => session.getAccessToken();

  setGetAccessToken(provider: () => string | null): void {
    this.getAccessToken = provider;
  }

  protected headers(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
      ...deviceHeaders(),
    };
    const token = this.getAccessToken();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    return headers;
  }
}
