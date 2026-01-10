import { mock } from 'bun:test';
import '@testing-library/dom';

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

// Mock fetch
global.fetch = mock(() => Promise.resolve(new Response()));

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
