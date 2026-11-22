const TEST_LOCALE = 'en-US';

const LOCALE_SENSITIVE_FORMATTERS = [
  'Collator',
  'DateTimeFormat',
  'DisplayNames',
  'ListFormat',
  'NumberFormat',
  'PluralRules',
  'RelativeTimeFormat',
  'Segmenter',
] as const;

type Callable = (...args: unknown[]) => unknown;

type Formatter = Callable & (new (...args: unknown[]) => object);

const formatters = Intl as unknown as Record<string, Formatter | undefined>;

function pinnedArguments([locales, ...rest]: unknown[]): unknown[] {
  return [locales ?? TEST_LOCALE, ...rest];
}

for (const name of LOCALE_SENSITIVE_FORMATTERS) {
  const native = formatters[name];
  if (native === undefined) {
    continue;
  }
  formatters[name] = new Proxy(native, {
    construct: (target, args, newTarget) =>
      Reflect.construct(target, pinnedArguments(args), newTarget),
    apply: (target, self, args) => Reflect.apply(target, self, pinnedArguments(args)),
  });
}

const LOCALE_SENSITIVE_METHODS: ReadonlyArray<readonly [object, string]> = [
  [Date.prototype, 'toLocaleString'],
  [Date.prototype, 'toLocaleDateString'],
  [Date.prototype, 'toLocaleTimeString'],
  [Number.prototype, 'toLocaleString'],
  [BigInt.prototype, 'toLocaleString'],
  [Array.prototype, 'toLocaleString'],
];

for (const [prototype, name] of LOCALE_SENSITIVE_METHODS) {
  const methods = prototype as Record<string, Callable>;
  const native = methods[name];
  Object.defineProperty(prototype, name, {
    configurable: true,
    writable: true,
    value(this: unknown, ...args: unknown[]) {
      return Reflect.apply(native, this, pinnedArguments(args));
    },
  });
}
