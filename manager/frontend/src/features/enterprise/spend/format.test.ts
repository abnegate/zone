import { describe, expect, it } from 'bun:test';
import { dateLabel, dayLabel, momentLabel, monthLabel } from './calendar';
import { RANGE_TOO_LONG } from './labels';
import { rangeMessage, refusalMessage } from './messages';
import {
  axisMoney,
  count,
  dollarsInput,
  exactMoney,
  exceedsMaximum,
  money,
  parseDollars,
} from './money';
import {
  MAXIMUM_DAYS,
  monthToDate,
  RangeProblem,
  rangeProblem,
  validDate,
  validRange,
} from './range';
import { SpendCode } from './schemas';

describe('money', () => {
  it('shows dollars and cents in en-US USD', () => {
    expect(money(0)).toBe('$0.00');
    expect(money(50_000_000)).toBe('$50.00');
    expect(money(1_234_567_890)).toBe('$1,234.57');
  });

  it('keeps a sub-cent amount visible instead of rounding it to zero', () => {
    expect(money(3_400)).toBe('$0.0034');
    expect(money(1)).toBe('$0.000001');
    expect(money(10_000)).toBe('$0.01');
  });

  it('shows the exact six-decimal amount', () => {
    expect(exactMoney(0)).toBe('$0.000000');
    expect(exactMoney(1_234_568)).toBe('$1.234568');
    expect(exactMoney(1_234_000_000_001)).toBe('$1,234,000.000001');
  });

  it('labels the chart axis compactly', () => {
    expect(axisMoney(20_000_000)).toBe('$20');
    expect(axisMoney(5_000)).toBe('$0.005');
    expect(axisMoney(2_000_000_000)).toBe('$2K');
  });

  it('counts with separators', () => {
    expect(count(1_234_567)).toBe('1,234,567');
  });
});

describe('dollarsInput', () => {
  it('writes a limit back as the shortest dollar amount that keeps every digit', () => {
    expect(dollarsInput(50_000_000)).toBe('50');
    expect(dollarsInput(12_500_000)).toBe('12.50');
    expect(dollarsInput(1_234_500)).toBe('1.2345');
    expect(dollarsInput(1)).toBe('0.000001');
  });

  it('round-trips through parseDollars', () => {
    for (const microdollars of [1, 999_999, 12_500_000, 50_000_000, 1_234_567_891]) {
      expect(parseDollars(dollarsInput(microdollars))).toBe(microdollars);
    }
  });
});

describe('parseDollars', () => {
  it('reads plain, signed and grouped dollar amounts as microdollars', () => {
    expect(parseDollars('50')).toBe(50_000_000);
    expect(parseDollars(' 12.5 ')).toBe(12_500_000);
    expect(parseDollars('$1,234.56')).toBe(1_234_560_000);
    expect(parseDollars('.25')).toBe(250_000);
    expect(parseDollars('0.000001')).toBe(1);
    expect(parseDollars('0')).toBe(0);
  });

  it('refuses anything that is not a whole number of microdollars', () => {
    for (const text of ['', '$', '.', '-5', '1e3', 'ten', '1.0000001', '12,34', '1,2345', '5.']) {
      expect(parseDollars(text), text).toBeNull();
    }
  });

  it('refuses an amount past what a number holds exactly', () => {
    expect(parseDollars('9007199254')).toBe(9_007_199_254_000_000);
    expect(parseDollars('9007199254.740991')).toBe(Number.MAX_SAFE_INTEGER);
    expect(parseDollars('9007199254.740992')).toBeNull();
    expect(parseDollars('9007199255')).toBeNull();
  });
});

describe('exceedsMaximum', () => {
  it('tells an amount too large to hold apart from one that is not an amount', () => {
    expect(exceedsMaximum('9007199254.740991')).toBe(false);
    expect(exceedsMaximum('9007199254.740992')).toBe(true);
    expect(exceedsMaximum('$99,999,999,999')).toBe(true);
    expect(exceedsMaximum('9'.repeat(400))).toBe(true);
    for (const text of ['', 'ten', '-5', '50']) {
      expect(exceedsMaximum(text), text).toBe(false);
    }
  });
});

describe('monthToDate', () => {
  it('runs from the first of the UTC month to today in UTC', () => {
    expect(monthToDate(new Date('2026-11-21T10:04:00Z'))).toEqual({
      from: '2026-11-01',
      to: '2026-11-21',
    });
  });

  it('uses the UTC day, not the local one, near midnight', () => {
    expect(monthToDate(new Date('2026-12-01T00:30:00Z'))).toEqual({
      from: '2026-12-01',
      to: '2026-12-01',
    });
    expect(monthToDate(new Date('2026-11-30T23:59:59Z'))).toEqual({
      from: '2026-11-01',
      to: '2026-11-30',
    });
  });
});

describe('validDate and validRange', () => {
  it('accepts only real calendar dates', () => {
    expect(validDate('2026-02-28')).toBe(true);
    expect(validDate('2028-02-29')).toBe(true);
    expect(validDate('2026-02-29')).toBe(false);
    expect(validDate('2026-13-01')).toBe(false);
    expect(validDate('2026-1-01')).toBe(false);
    expect(validDate('')).toBe(false);
  });

  it('accepts a single day and refuses a start after the end', () => {
    expect(validRange({ from: '2026-11-05', to: '2026-11-05' })).toBe(true);
    expect(validRange({ from: '2026-11-01', to: '2026-11-21' })).toBe(true);
    expect(validRange({ from: '2026-11-22', to: '2026-11-21' })).toBe(false);
    expect(validRange({ from: '', to: '2026-11-21' })).toBe(false);
  });

  it(`accepts at most ${MAXIMUM_DAYS} days, counting both ends, like the server`, () => {
    expect(MAXIMUM_DAYS).toBe(366);
    expect(validRange({ from: '2024-01-01', to: '2024-12-31' })).toBe(true);
    expect(validRange({ from: '2025-01-01', to: '2026-01-01' })).toBe(true);
    expect(validRange({ from: '2025-01-01', to: '2026-01-02' })).toBe(false);
    expect(validRange({ from: '2020-01-01', to: '2026-11-21' })).toBe(false);
  });
});

describe('rangeProblem', () => {
  it('names what is wrong with a range', () => {
    expect(rangeProblem({ from: '2026-11-01', to: '2026-11-21' })).toBeNull();
    expect(rangeProblem({ from: '2026-11-22', to: '2026-11-21' })).toBe(RangeProblem.Invalid);
    expect(rangeProblem({ from: '2026-02-30', to: '2026-03-01' })).toBe(RangeProblem.Invalid);
    expect(rangeProblem({ from: '2025-01-01', to: '2026-01-02' })).toBe(RangeProblem.TooLong);
  });

  it('explains each problem in its own words', () => {
    expect(rangeMessage(RangeProblem.Invalid)).toBe(refusalMessage(SpendCode.RangeInvalid));
    expect(rangeMessage(RangeProblem.TooLong)).toBe(RANGE_TOO_LONG);
    expect(RANGE_TOO_LONG).toContain(String(MAXIMUM_DAYS));
  });
});

describe('calendar labels', () => {
  it('names days, months and instants in UTC', () => {
    expect(dayLabel('2026-11-01')).toBe('Nov 1');
    expect(dateLabel('2026-11-01')).toBe('Nov 1, 2026');
    expect(monthLabel('2026-11')).toBe('November 2026');
    expect(momentLabel('2026-11-21T10:04:00Z')).toBe('Nov 21 at 10:04 AM UTC');
  });

  it('passes through a value it cannot read', () => {
    expect(momentLabel('soon')).toBe('soon');
  });
});
