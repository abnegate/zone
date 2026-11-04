import { describe, expect, it } from 'bun:test';
import { act, render, screen } from '@testing-library/react';
import { Suspense } from 'react';
import { preload } from '../routes';
import { lazyRoute } from './lazyRoute';

const page = (text: string) => () => Promise.resolve({ default: () => <p>{text}</p> });

describe('lazyRoute', () => {
  it('renders a preloaded route on the first pass without its fallback', async () => {
    const load = page('ready');
    await preload(load);
    const Route = lazyRoute(load);

    render(
      <Suspense fallback={<p>waiting</p>}>
        <Route />
      </Suspense>
    );

    expect(screen.getByText('ready')).toBeInTheDocument();
    expect(screen.queryByText('waiting')).toBeNull();
  });

  it('falls back and then renders a route that was not preloaded', async () => {
    const Route = lazyRoute(page('later'));

    render(
      <Suspense fallback={<p>waiting</p>}>
        <Route />
      </Suspense>
    );

    expect(screen.getByText('waiting')).toBeInTheDocument();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve));
    });
    expect(screen.getByText('later')).toBeInTheDocument();
  });
});
