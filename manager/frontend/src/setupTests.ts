import { afterEach, expect, mock, vi } from 'bun:test';
import '@testing-library/dom';
import { cleanup } from '@testing-library/react';

if (typeof globalThis.HTMLFormElement === 'undefined') {
  globalThis.HTMLFormElement = window.HTMLFormElement;
}

// Polyfill NodeFilter for Radix UI's focus-scope (uses document.createTreeWalker)
if (typeof globalThis.NodeFilter === 'undefined') {
  (globalThis as Record<string, unknown>).NodeFilter = {
    SHOW_ELEMENT: 1,
    SHOW_TEXT: 4,
    FILTER_ACCEPT: 1,
    FILTER_REJECT: 2,
    FILTER_SKIP: 3,
  };
}

// Cleanup after each test to prevent DOM accumulation.
afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

// Extend expect with jest-dom-like matchers
expect.extend({
  toBeInTheDocument(received: Element | null) {
    const pass = received !== null && document.body.contains(received);
    return {
      pass,
      message: () =>
        pass
          ? 'expected element not to be in the document'
          : 'expected element to be in the document',
    };
  },
  toHaveClass(received: Element | null, className: string) {
    const pass = received?.classList.contains(className) ?? false;
    return {
      pass,
      message: () =>
        pass
          ? `expected element not to have class "${className}"`
          : `expected element to have class "${className}"`,
    };
  },
  toHaveAttribute(received: Element | null, attr: string, value?: string) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const hasAttr = received.hasAttribute(attr);
    const attrValue = received.getAttribute(attr);
    const pass = value !== undefined ? attrValue === value : hasAttr;
    return {
      pass,
      message: () =>
        pass
          ? `expected element not to have attribute "${attr}"${value !== undefined ? ` with value "${value}"` : ''}`
          : `expected element to have attribute "${attr}"${value !== undefined ? ` with value "${value}"` : ''}`,
    };
  },
  toHaveValue(
    received: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | null,
    value: string | number
  ) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const pass = received.value === String(value);
    return {
      pass,
      message: () =>
        pass
          ? `expected element not to have value "${value}"`
          : `expected element to have value "${value}", but got "${received.value}"`,
    };
  },
  toBeDisabled(received: HTMLElement | null) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const pass = (received as HTMLButtonElement).disabled === true;
    return {
      pass,
      message: () =>
        pass ? 'expected element not to be disabled' : 'expected element to be disabled',
    };
  },
  toBeEnabled(received: HTMLElement | null) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const pass = (received as HTMLButtonElement).disabled !== true;
    return {
      pass,
      message: () =>
        pass ? 'expected element not to be enabled' : 'expected element to be enabled',
    };
  },
  toBeChecked(received: HTMLInputElement | null) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const pass = received.checked === true;
    return {
      pass,
      message: () =>
        pass ? 'expected element not to be checked' : 'expected element to be checked',
    };
  },
  toContainHTML(received: Element | null, html: string) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const pass = received.innerHTML.includes(html);
    return {
      pass,
      message: () =>
        pass
          ? `expected element not to contain HTML "${html}"`
          : `expected element to contain HTML "${html}"`,
    };
  },
  toHaveTextContent(received: Element | null, text: string | RegExp) {
    if (received === null) {
      return { pass: false, message: () => 'element is null' };
    }
    const actual = received.textContent ?? '';
    const pass = typeof text === 'string' ? actual.includes(text) : text.test(actual);
    return {
      pass,
      message: () =>
        pass
          ? `expected element not to have text content "${text}"`
          : `expected element to have text content "${text}"`,
    };
  },
});

// Mock window.matchMedia globally for all tests
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: mock((query: string) => ({
    matches: !query.includes('dark'),
    media: query,
    onchange: null,
    addEventListener: mock(() => {}),
    removeEventListener: mock(() => {}),
    addListener: mock(() => {}),
    removeListener: mock(() => {}),
    dispatchEvent: mock(() => false),
  })),
});

// Mock fetch with a minimal response to avoid open body streams.
global.fetch = mock(() =>
  Promise.resolve({
    ok: true,
    status: 200,
    json: async () => ({}),
    text: async () => '',
  } as Response)
);

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
  };
})();

Object.defineProperty(window, 'localStorage', { value: localStorageMock });
Object.defineProperty(globalThis, 'localStorage', { value: localStorageMock });

// Mock sessionStorage
const sessionStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
  };
})();

Object.defineProperty(window, 'sessionStorage', { value: sessionStorageMock });
