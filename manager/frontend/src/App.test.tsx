import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { render, screen } from '@testing-library/react';
import App from './App';

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
