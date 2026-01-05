export interface RetryOptions {
  maxAttempts?: number;
  initialDelay?: number;
  maxDelay?: number;
  backoffMultiplier?: number;
  timeout?: number;
  onRetry?: (attempt: number, error: Error) => void;
}

export class RetryError extends Error {
  constructor(
    message: string,
    public readonly attempts: number,
    public readonly lastError: Error
  ) {
    super(message);
    this.name = 'RetryError';
  }
}

export async function withRetry<T>(
  fn: (signal: AbortSignal) => Promise<T>,
  options: RetryOptions = {}
): Promise<T> {
  const {
    maxAttempts = 3,
    initialDelay = 1000,
    maxDelay = 10000,
    backoffMultiplier = 2,
    timeout = 30000,
    onRetry,
  } = options;

  let lastError: Error = new Error('Unknown error');

  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), timeout);

    try {
      const result = await fn(controller.signal);
      clearTimeout(timeoutId);
      return result;
    } catch (error) {
      clearTimeout(timeoutId);
      lastError = error instanceof Error ? error : new Error(String(error));

      // Don't retry on abort
      if (controller.signal.aborted) {
        throw new RetryError(`Request timed out after ${timeout}ms`, attempt, lastError);
      }

      // Don't retry on final attempt
      if (attempt === maxAttempts) {
        throw new RetryError(
          `Failed after ${maxAttempts} attempts: ${lastError.message}`,
          attempt,
          lastError
        );
      }

      // Calculate delay with exponential backoff
      const delay = Math.min(initialDelay * backoffMultiplier ** (attempt - 1), maxDelay);

      onRetry?.(attempt, lastError);
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }

  throw new RetryError(`Failed after ${maxAttempts} attempts`, maxAttempts, lastError);
}
