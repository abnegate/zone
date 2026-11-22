import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import { Outlet } from 'react-router-dom';
import { WaitingNotifier } from '../../features/chats/components';
import { PullProvider } from '../../features/models/hooks/usePull';
import { TrainProvider } from '../../features/models/hooks/useTrain';

export default function AuthenticatedProviders() {
  const [queryClient] = useState(() => new QueryClient());

  return (
    <QueryClientProvider client={queryClient}>
      <PullProvider>
        <TrainProvider>
          {/* Every signed-in page, because a chat stopping matters most to a
              reader who is somewhere else. It renders nothing. */}
          <WaitingNotifier />
          <Outlet />
        </TrainProvider>
      </PullProvider>
    </QueryClientProvider>
  );
}
