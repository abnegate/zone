import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Outlet } from 'react-router-dom';
import { PullProvider } from '../../features/models/hooks/usePull';
import { TrainProvider } from '../../features/models/hooks/useTrain';

const queryClient = new QueryClient();

export default function AuthenticatedProviders() {
  return (
    <QueryClientProvider client={queryClient}>
      <PullProvider>
        <TrainProvider>
          <Outlet />
        </TrainProvider>
      </PullProvider>
    </QueryClientProvider>
  );
}
