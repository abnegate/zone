import { ApiError } from '../../api/ApiError';
import { deviceHeaders } from '../../api/device';
import { HttpStatus } from '../../api/HttpStatus';
import { session } from '../../api/session';

export const REFRESH = '1';

export abstract class GitHubApi {
  private getAccessToken: () => string | null = () => session.getAccessToken();

  setGetAccessToken(provider: () => string | null): void {
    this.getAccessToken = provider;
  }

  protected abstract refusal(body: unknown): ApiError | null;

  protected async send(url: string, init: RequestInit, action: string): Promise<unknown> {
    const response = await fetch(url, { ...init, headers: this.headers() });
    if (!response.ok) {
      throw await this.failure(response, action);
    }
    return response.json();
  }

  private headers(): HeadersInit {
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

  private async failure(response: Response, action: string): Promise<ApiError> {
    if (response.status === HttpStatus.Conflict) {
      const body: unknown = await response
        .clone()
        .json()
        .catch(() => undefined);
      const refused = this.refusal(body);
      if (refused) return refused;
    }
    return ApiError.from(response, action);
  }
}
