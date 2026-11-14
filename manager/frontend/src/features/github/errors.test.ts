import { describe, expect, it } from 'bun:test';
import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';
import { errorMessage, outcomeUnknown } from './errors';

describe('errorMessage', () => {
  it('reports nothing without an error', () => {
    expect(errorMessage(null, 'Failed')).toBeNull();
  });

  it('reports the message of an error', () => {
    expect(errorMessage(new Error('Network down'), 'Failed')).toBe('Network down');
  });

  it('falls back for something thrown that is not an error', () => {
    expect(errorMessage('broken', 'Failed')).toBe('Failed');
  });
});

describe('outcomeUnknown', () => {
  it('reads a 500 as a write whose outcome is unknown', () => {
    expect(outcomeUnknown(new ApiError('Stopped', HttpStatus.InternalServerError))).toBe(true);
  });

  it('reads any other failure as known', () => {
    expect(outcomeUnknown(new ApiError('Gateway', HttpStatus.BadGateway))).toBe(false);
    expect(outcomeUnknown(new ApiError('Refused', HttpStatus.Conflict))).toBe(false);
    expect(outcomeUnknown(new Error('Network down'))).toBe(false);
    expect(outcomeUnknown(null)).toBe(false);
  });
});
