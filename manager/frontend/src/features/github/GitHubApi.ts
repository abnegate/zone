import { ApiError } from '../../api/ApiError';
import { AuthorizedApi } from '../../api/AuthorizedApi';
import { HttpStatus } from '../../api/HttpStatus';

export const REFRESH = '1';

export abstract class GitHubApi extends AuthorizedApi {
  protected abstract refusal(body: unknown): ApiError | null;

  protected async send(url: string, init: RequestInit, action: string): Promise<unknown> {
    const response = await fetch(url, { ...init, headers: this.headers() });
    if (!response.ok) {
      throw await this.failure(response, action);
    }
    return response.json();
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
