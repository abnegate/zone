export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }

  static async from(response: Response, action: string): Promise<ApiError> {
    const reason = await ApiError.reason(response);
    return new ApiError(`${action}: ${reason ?? response.status}`, response.status);
  }

  private static async reason(response: Response): Promise<string | undefined> {
    try {
      const body = JSON.parse(await response.text()) as { error?: unknown; message?: unknown };
      return [body.error, body.message].find(
        (value): value is string => typeof value === 'string' && value.length > 0
      );
    } catch {
      return undefined;
    }
  }
}
