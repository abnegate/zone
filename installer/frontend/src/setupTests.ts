import { mock, expect, afterEach, jest } from 'bun:test';
import '@testing-library/dom';
import { cleanup } from '@testing-library/react';

// Cleanup after each test to prevent DOM accumulation
afterEach(() => {
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
          ? `expected element not to be in the document`
          : `expected element to be in the document`,
    };
  },
  toHaveClass(received: Element | null, className: string) {
    const pass = received !== null && received.classList.contains(className);
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
  toHaveValue(received: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | null, value: string | number) {
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
        pass
          ? `expected element not to be disabled`
          : `expected element to be disabled`,
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
        pass
          ? `expected element not to be checked`
          : `expected element to be checked`,
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

// Expose Bun's jest-compatible helpers for legacy tests.
(globalThis as Record<string, unknown>).jest = jest;

// Mock window.matchMedia
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: mock((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: mock(() => {}),
    removeEventListener: mock(() => {}),
    addListener: mock(() => {}),
    removeListener: mock(() => {}),
    dispatchEvent: mock(() => false),
  })),
});

// Mock crypto.subtle for Web Crypto API tests
const mockCryptoKey = {} as CryptoKey;

Object.defineProperty(global, 'crypto', {
  value: {
    getRandomValues: (arr: Uint8Array) => {
      for (let i = 0; i < arr.length; i++) {
        arr[i] = Math.floor(Math.random() * 256);
      }
      return arr;
    },
    subtle: {
      generateKey: mock(() => Promise.resolve(mockCryptoKey)),
      encrypt: mock(() => Promise.resolve(new ArrayBuffer(32))),
      decrypt: mock(() => Promise.resolve(new TextEncoder().encode('decrypted'))),
      importKey: mock(() => Promise.resolve(mockCryptoKey)),
      exportKey: mock(() => Promise.resolve({ kty: 'oct', k: 'test' })),
    },
  },
});

// Mock fetch
global.fetch = mock(() => Promise.resolve(new Response()));

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
