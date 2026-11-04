import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { render, screen } from '@testing-library/react';
import RouteErrorBoundary from './RouteErrorBoundary';

function Broken(): never {
  throw new Error('Failed to fetch dynamically imported module');
}

describe('RouteErrorBoundary', () => {
  let consoleError: ReturnType<typeof spyOn>;

  beforeEach(() => {
    consoleError = spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    consoleError.mockRestore();
  });

  it('renders its children when nothing fails', () => {
    render(
      <RouteErrorBoundary>
        <p>page</p>
      </RouteErrorBoundary>
    );
    expect(screen.getByText('page')).toBeInTheDocument();
  });

  it('offers a reload when a route fails to load', () => {
    render(
      <RouteErrorBoundary>
        <Broken />
      </RouteErrorBoundary>
    );
    expect(screen.getByRole('alert')).toHaveTextContent('This page could not load.');
    expect(screen.getByRole('button', { name: 'Reload' })).toBeInTheDocument();
  });
});
