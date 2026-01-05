import { RetryError, withRetry } from './retry';

describe('withRetry', () => {
  it('returns result on success', async () => {
    const fn = jest.fn().mockResolvedValue('success');

    const result = await withRetry(fn);

    expect(result).toBe('success');
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('retries on failure', async () => {
    const fn = jest.fn().mockRejectedValueOnce(new Error('fail')).mockResolvedValueOnce('success');

    const result = await withRetry(fn, { initialDelay: 1, maxAttempts: 3 });

    expect(result).toBe('success');
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('throws RetryError after max attempts', async () => {
    const fn = jest.fn().mockRejectedValue(new Error('always fails'));

    await expect(withRetry(fn, { maxAttempts: 2, initialDelay: 1 })).rejects.toThrow(RetryError);

    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('calls onRetry callback', async () => {
    const fn = jest.fn().mockRejectedValueOnce(new Error('fail')).mockResolvedValueOnce('success');
    const onRetry = jest.fn();

    await withRetry(fn, { initialDelay: 1, onRetry });

    expect(onRetry).toHaveBeenCalledWith(1, expect.any(Error));
  });

  it('passes abort signal to function', async () => {
    const fn = jest.fn().mockResolvedValue('success');

    await withRetry(fn);

    expect(fn).toHaveBeenCalledWith(expect.any(AbortSignal));
  });
});

describe('RetryError', () => {
  it('stores attempts and lastError', () => {
    const lastError = new Error('last error');
    const error = new RetryError('Failed', 3, lastError);

    expect(error.attempts).toBe(3);
    expect(error.lastError).toBe(lastError);
    expect(error.message).toBe('Failed');
    expect(error.name).toBe('RetryError');
  });
});
