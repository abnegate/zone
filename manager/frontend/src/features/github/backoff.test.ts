import { describe, expect, it } from 'bun:test';
import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';
import {
  GATEWAY_BACKOFF_MS,
  gatewayBackoff,
  gatewayFailed,
  notFound,
  RETRY_LIMIT,
  retryDelay,
  retryPolled,
  retryRead,
} from './backoff';

const gateway = new ApiError('Failed to fetch releases: 502', HttpStatus.BadGateway);
const missing = new ApiError('Failed to fetch releases: 404', HttpStatus.NotFound);
const refused = new ApiError('The head moved', HttpStatus.Conflict);
const malformed = new ApiError('Invalid query', HttpStatus.BadRequest);
const forbidden = new ApiError('Missing permission', HttpStatus.Forbidden);
const network = new Error('Network down');

describe('gateway backoff', () => {
  it('recognises only a 502 api error as a gateway failure', () => {
    expect(gatewayFailed(gateway)).toBe(true);
    expect(gatewayFailed(missing)).toBe(false);
    expect(gatewayFailed(refused)).toBe(false);
    expect(gatewayFailed(new ApiError('Internal', 500))).toBe(false);
    expect(gatewayFailed(network)).toBe(false);
    expect(gatewayFailed(null)).toBe(false);
  });

  it('waits 10, 20, 40, 80 and then 120 seconds between failures', () => {
    expect([1, 2, 3, 4, 5].map(gatewayBackoff)).toEqual([...GATEWAY_BACKOFF_MS]);
    expect(GATEWAY_BACKOFF_MS).toEqual([10_000, 20_000, 40_000, 80_000, 120_000]);
  });

  it('stays at the longest wait after five failures', () => {
    expect(gatewayBackoff(6)).toBe(120_000);
    expect(gatewayBackoff(50)).toBe(120_000);
  });

  it('treats a count below one as the first failure', () => {
    expect(gatewayBackoff(0)).toBe(10_000);
  });
});

describe('retryPolled', () => {
  it('keeps retrying while GitHub is unreachable', () => {
    expect(retryPolled(0, gateway)).toBe(true);
    expect(retryPolled(50, gateway)).toBe(true);
  });

  it('retries any other failure up to the limit', () => {
    expect(retryPolled(0, network)).toBe(true);
    expect(retryPolled(RETRY_LIMIT - 1, network)).toBe(true);
    expect(retryPolled(RETRY_LIMIT, network)).toBe(false);
  });

  it('never retries what is missing or was refused', () => {
    expect(retryPolled(0, missing)).toBe(false);
    expect(retryPolled(0, refused)).toBe(false);
  });
});

describe('retryRead', () => {
  it('gives up on a 502 like any other failure', () => {
    expect(retryRead(0, gateway)).toBe(true);
    expect(retryRead(RETRY_LIMIT, gateway)).toBe(false);
  });

  it('never retries what is missing or was refused', () => {
    expect(retryRead(0, missing)).toBe(false);
    expect(retryRead(0, refused)).toBe(false);
  });

  it('never retries a malformed or forbidden request', () => {
    expect(retryRead(0, malformed)).toBe(false);
    expect(retryRead(0, forbidden)).toBe(false);
    expect(retryPolled(0, malformed)).toBe(false);
    expect(retryPolled(0, forbidden)).toBe(false);
  });
});

describe('notFound', () => {
  it('recognises only a 404 api error', () => {
    expect(notFound(missing)).toBe(true);
    expect(notFound(gateway)).toBe(false);
    expect(notFound(network)).toBe(false);
    expect(notFound(null)).toBe(false);
  });
});

describe('retryDelay', () => {
  it('escalates 10, 20, 40, 80 then every 120 seconds across consecutive 502s', () => {
    const delays = [0, 1, 2, 3, 4, 5, 9].map((failures) => retryDelay(failures, gateway));
    expect(delays).toEqual([10_000, 20_000, 40_000, 80_000, 120_000, 120_000, 120_000]);
  });

  it('doubles from a second up to thirty seconds otherwise', () => {
    expect(retryDelay(0, network)).toBe(1_000);
    expect(retryDelay(1, network)).toBe(2_000);
    expect(retryDelay(10, network)).toBe(30_000);
  });
});
