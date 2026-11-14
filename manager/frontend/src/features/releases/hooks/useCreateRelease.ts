import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { releasesApi } from '../../../api/releases';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { NO_WORKSPACE, outcomeUnknown } from '../../github';
import { pullRequestListings } from '../../pull-requests';
import { ReleaseRefusal } from '../ReleaseRefusal';
import type { Created, CreateReleaseRequest, Preview, PreviewRequest } from '../types';
import { releasesKey } from './keys';

interface Submission<Request> {
  workspaceId: string;
  request: Request;
}

export interface CreateRelease {
  preview: (request: PreviewRequest) => Promise<Preview>;
  previewing: boolean;
  create: (request: CreateReleaseRequest) => Promise<Created>;
  creating: boolean;
  reset: () => void;
}

function mayHaveWritten(error: Error): boolean {
  return (
    (error instanceof ReleaseRefusal && error.reconciliation !== null) || outcomeUnknown(error)
  );
}

export function useCreateRelease(): CreateRelease {
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const invalidate = useCallback(
    (target: string) =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: releasesKey(target) }),
        queryClient.invalidateQueries(pullRequestListings(target)),
      ]),
    [queryClient]
  );

  const previewMutation = useMutation({
    mutationFn: ({ workspaceId: target, request }: Submission<PreviewRequest>) =>
      releasesApi.preview(target, request),
  });

  const createMutation = useMutation({
    mutationFn: ({ workspaceId: target, request }: Submission<CreateReleaseRequest>) =>
      releasesApi.create(target, request),
    onSuccess: (_created, { workspaceId: target }) => {
      void invalidate(target);
    },
    onError: (error, { workspaceId: target }) => {
      if (mayHaveWritten(error)) void invalidate(target);
    },
  });

  const { mutateAsync: submitPreview, reset: resetPreview } = previewMutation;
  const { mutateAsync: submitCreate, reset: resetCreate } = createMutation;

  const preview = useCallback(
    (request: PreviewRequest) => {
      if (!workspaceId) return Promise.reject(new Error(NO_WORKSPACE));
      return submitPreview({ workspaceId, request });
    },
    [submitPreview, workspaceId]
  );

  const create = useCallback(
    (request: CreateReleaseRequest) => {
      if (!workspaceId) return Promise.reject(new Error(NO_WORKSPACE));
      return submitCreate({ workspaceId, request });
    },
    [submitCreate, workspaceId]
  );

  const reset = useCallback(() => {
    resetPreview();
    resetCreate();
  }, [resetPreview, resetCreate]);

  return {
    preview,
    previewing: previewMutation.isPending,
    create,
    creating: createMutation.isPending,
    reset,
  };
}
