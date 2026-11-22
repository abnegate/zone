export class ApiError extends Error {
  readonly status: number;
  readonly code: string | undefined;

  constructor(message: string, status: number, code?: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
  }

  static async from(response: Response, action: string): Promise<ApiError> {
    const reason = await ApiError.reason(response);
    return new ApiError(`${action}: ${reason ?? response.status}`, response.status);
  }

  static async body(response: Response): Promise<unknown> {
    try {
      return await response.json();
    } catch {
      return undefined;
    }
  }

  static refusal(body: unknown): { reason: string | undefined; code: string | undefined } {
    const details: { message?: unknown; error?: unknown; detail?: unknown; code?: unknown } =
      typeof body === 'object' && body !== null ? body : {};
    return {
      reason: [details.message, details.error, details.detail].find(ApiError.filled),
      code: ApiError.filled(details.code) ? details.code : undefined,
    };
  }

  static async refused(response: Response, action: string): Promise<ApiError> {
    const { reason, code } = ApiError.refusal(await ApiError.body(response));
    return new ApiError(reason ?? `${action}: ${response.status}`, response.status, code);
  }

  private static filled(value: unknown): value is string {
    return typeof value === 'string' && value.length > 0;
  }

  private static async reason(response: Response): Promise<string | undefined> {
    try {
      const body = JSON.parse(await response.text()) as { error?: unknown; message?: unknown };
      return [body.error, body.message].find(ApiError.filled);
    } catch {
      return undefined;
    }
  }
}
