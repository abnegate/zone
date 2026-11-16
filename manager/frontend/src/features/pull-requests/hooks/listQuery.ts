import { queryOptions } from '@tanstack/react-query';
import { pullRequestsApi } from '../../../api/pullRequests';
import { NO_WORKSPACE } from '../../github/errors';
import { pullRequestKeys } from '../queryKeys';

export function pullRequestListQuery(workspaceId: string | undefined) {
  return queryOptions({
    queryKey: pullRequestKeys.list(workspaceId),
    queryFn: () => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return pullRequestsApi.list(workspaceId, false);
    },
  });
}
