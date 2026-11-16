import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import App from './App';
import { SSO_INCOMPLETE } from './features/auth/ssoMessages';

describe('App', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it('renders without crashing', () => {
    render(<App />);
    expect(document.body).toBeInTheDocument();
  });

  it('serves the single sign-on callback as a public page', async () => {
    window.history.replaceState(null, '', '/sso/callback?code=code-1&state=state-1');
    const router = ({ children }: { children: ReactNode }) => (
      <MemoryRouter initialEntries={['/sso/callback?code=code-1&state=state-1']}>
        {children}
      </MemoryRouter>
    );
    try {
      render(<App router={router} />);

      expect(await screen.findByText(SSO_INCOMPLETE)).toBeInTheDocument();
      expect(window.location.search).toBe('');
    } finally {
      window.history.replaceState(null, '', '/');
    }
  });

  it('offers a reload when a provider throws on the first render', () => {
    const getItem = spyOn(localStorage, 'getItem').mockImplementation(() => {
      throw new Error('storage unavailable');
    });
    const consoleError = spyOn(console, 'error').mockImplementation(() => {});
    try {
      render(<App />);
      expect(screen.getByRole('alert')).toHaveTextContent('This page could not load.');
      expect(screen.getByRole('button', { name: 'Reload' })).toBeInTheDocument();
    } finally {
      getItem.mockRestore();
      consoleError.mockRestore();
    }
  });
});
