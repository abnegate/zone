import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { type QueryClient, useQuery, useQueryClient } from '@tanstack/react-query';
import { act, render, screen } from '@testing-library/react';
import { StrictMode } from 'react';

const load = mock(() => new Promise<never>(() => {}));

let client: QueryClient | null = null;
let reading = false;

function Loader() {
  useQuery({ queryKey: ['reader'], queryFn: load });
  return null;
}

function Reader() {
  client = useQueryClient();
  return <div data-testid="outlet">{reading && <Loader />}</div>;
}

mock.module('react-router-dom', () => ({
  Outlet: Reader,
}));

mock.module('../../features/models/hooks/usePull', () => ({
  PullProvider: ({ children }: { children: React.ReactNode }) => children,
}));

mock.module('../../features/models/hooks/useTrain', () => ({
  TrainProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// The notifier reads the workspace and polls what its chats are doing; what it
// does is its own tests' business, and this one's is that every signed-in page
// mounts it.
mock.module('../../features/chats/components', () => ({
  WaitingNotifier: () => <div data-testid="waiting-notifier" />,
}));

let AuthenticatedProviders: typeof import('./AuthenticatedProviders').default;

beforeAll(async () => {
  AuthenticatedProviders = (await import('./AuthenticatedProviders')).default;
});

afterAll(() => {
  mock.restore();
});

describe('AuthenticatedProviders', () => {
  beforeEach(() => {
    load.mockClear();
    reading = false;
  });

  it('watches for a chat that stops and waits, on every signed-in page', () => {
    render(<AuthenticatedProviders />);

    expect(screen.getByTestId('waiting-notifier')).toBeInTheDocument();
    expect(screen.getByTestId('outlet')).toBeInTheDocument();
  });

  it("forgets the signed-out reader's data", () => {
    const first = render(<AuthenticatedProviders />);
    client?.setQueryData(['pull-requests', 'workspace-1', 'list'], { pull_requests: [] });
    first.unmount();

    render(<AuthenticatedProviders />);

    expect(client?.getQueryCache().getAll()).toHaveLength(0);
  });

  it('keeps a read that starts on the first render when strict mode remounts it', async () => {
    reading = true;
    render(
      <StrictMode>
        <AuthenticatedProviders />
      </StrictMode>
    );
    await act(async () => {});

    expect(load).toHaveBeenCalledTimes(1);
    expect(client?.getQueryCache().find({ queryKey: ['reader'] })?.state.fetchStatus).toBe(
      'fetching'
    );
  });
});
