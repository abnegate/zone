const LOCALE = 'en-US';

const CURRENCY = 'USD';

export const MICRODOLLARS_PER_DOLLAR = 1_000_000;

const MICRODOLLARS_PER_CENT = 10_000;

const MAXIMUM_MICRODOLLARS = Number.MAX_SAFE_INTEGER;

const FRACTION_DIGITS = 6;

const CENT_DIGITS = 2;

const SIGNIFICANT_DIGITS = 3;

const AMOUNT = /^\$?\s*(\d{1,3}(?:,\d{3})+|\d*)(?:\.(\d{1,6}))?$/;

const THOUSANDS = /,/g;

const TRAILING_ZEROS = /0+$/;

const cents = new Intl.NumberFormat(LOCALE, {
  style: 'currency',
  currency: CURRENCY,
  minimumFractionDigits: CENT_DIGITS,
  maximumFractionDigits: CENT_DIGITS,
});

const fractions = new Intl.NumberFormat(LOCALE, {
  style: 'currency',
  currency: CURRENCY,
  minimumFractionDigits: CENT_DIGITS,
  maximumFractionDigits: FRACTION_DIGITS,
});

const compact = new Intl.NumberFormat(LOCALE, {
  style: 'currency',
  currency: CURRENCY,
  notation: 'compact',
  maximumSignificantDigits: SIGNIFICANT_DIGITS,
});

const wholes = new Intl.NumberFormat(LOCALE, { maximumFractionDigits: 0 });

const counts = new Intl.NumberFormat(LOCALE);

function split(microdollars: number): { whole: number; fraction: string } {
  const whole = Math.floor(microdollars / MICRODOLLARS_PER_DOLLAR);
  const fraction = String(microdollars % MICRODOLLARS_PER_DOLLAR).padStart(FRACTION_DIGITS, '0');
  return { whole, fraction };
}

export function money(microdollars: number): string {
  const dollars = microdollars / MICRODOLLARS_PER_DOLLAR;
  const fine = microdollars > 0 && microdollars < MICRODOLLARS_PER_CENT;
  return (fine ? fractions : cents).format(dollars);
}

export function exactMoney(microdollars: number): string {
  const { whole, fraction } = split(microdollars);
  return `$${wholes.format(whole)}.${fraction}`;
}

export function axisMoney(microdollars: number): string {
  return compact.format(microdollars / MICRODOLLARS_PER_DOLLAR);
}

export function count(value: number): string {
  return counts.format(value);
}

export function dollarsInput(microdollars: number): string {
  const { whole, fraction } = split(microdollars);
  const trimmed = fraction.replace(TRAILING_ZEROS, '');
  if (trimmed === '') return String(whole);
  return `${whole}.${trimmed.padEnd(CENT_DIGITS, '0')}`;
}

function read(text: string): number | null {
  const match = AMOUNT.exec(text.trim());
  if (match === null) return null;
  const [, whole = '', fraction = ''] = match;
  if (whole === '' && fraction === '') return null;
  return (
    Number(whole.replace(THOUSANDS, '') || '0') * MICRODOLLARS_PER_DOLLAR +
    Number(fraction.padEnd(FRACTION_DIGITS, '0'))
  );
}

export function parseDollars(text: string): number | null {
  const microdollars = read(text);
  return microdollars !== null && microdollars <= MAXIMUM_MICRODOLLARS ? microdollars : null;
}

export function exceedsMaximum(text: string): boolean {
  const microdollars = read(text);
  return microdollars !== null && microdollars > MAXIMUM_MICRODOLLARS;
}
