import { mock } from 'bun:test';
import '@testing-library/dom';

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
