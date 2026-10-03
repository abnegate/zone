import { describe, expect, it } from 'bun:test';
import { formatEta, trainHeadline, trainPercent } from './trainProgress';

describe('trainProgress', () => {
  it('formats an eta in minutes or hours', () => {
    expect(formatEta(12)).toBe('less than a minute left');
    expect(formatEta(60)).toBe('about 1 minute left');
    expect(formatEta(180)).toBe('about 3 minutes left');
    expect(formatEta(3600)).toBe('about 1 hour left');
    expect(formatEta(7200)).toBe('about 2 hours left');
  });

  it('turns a step into a percentage', () => {
    expect(trainPercent(12, 400)).toBe(3);
    expect(trainPercent(400, 400)).toBe(100);
    expect(trainPercent(null, 400)).toBeNull();
    expect(trainPercent(0, 400)).toBe(0);
  });

  it('names the overlay from the job status', () => {
    expect(trainHeadline('running', 'jerry')).toBe('Training jerry');
    expect(trainHeadline('succeeded', 'jerry')).toBe('Training finished jerry');
    expect(trainHeadline('failed', 'jerry')).toBe('Training failed jerry');
  });
});
