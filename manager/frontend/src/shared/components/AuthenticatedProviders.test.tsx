import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';
import { type QueryClient, useQueryClient } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';

let client: QueryClient | null = null;

function Reader() {
  client = useQueryClient();
  return <div data-testid="outlet" />;
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
});
