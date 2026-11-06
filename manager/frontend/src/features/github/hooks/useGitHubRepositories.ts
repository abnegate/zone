import {
  type InfiniteData,
  skipToken,
  useInfiniteQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import { type GitHubRepositorySearch, githubApi } from '../../../api/github';
import { REPOSITORY_PAGE_SIZE } from '../constants';
import type { GitHubRepositoriesResponse, GitHubRepositoryInstallation } from '../types';
import { githubQueryKeys } from './githubQueryKeys';

const NO_INSTALLATIONS: GitHubRepositoryInstallation[] = [];

export function useGitHubRepositories(organizationId: string | null, query: string) {
  const queryClient = useQueryClient();
  const queryKey = githubQueryKeys.repositories(organizationId, query);

  const search = useCallback(
    (page: number, refresh = false): GitHubRepositorySearch => ({
      query,
      page,
      perPage: REPOSITORY_PAGE_SIZE,
      ...(refresh ? { refresh } : {}),
    }),
    [query]
  );

  const {
    data,
    isPending,
    isPlaceholderData,
    error,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
    refetch: refetchPages,
  } = useInfiniteQuery({
    queryKey,
    queryFn:
      organizationId === null
        ? skipToken
        : ({ pageParam }) => githubApi.listRepositories(organizationId, search(pageParam)),
    initialPageParam: 1,
    getNextPageParam: (last) => last.next_page ?? undefined,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[1] === organizationId ? previous : undefined,
  });

  const repositories = useMemo(
    () => data?.pages.flatMap((page) => page.repositories) ?? [],
    [data]
  );

  const loadMore = useCallback(async (): Promise<void> => {
    if (!hasNextPage || isFetchingNextPage) {
      return;
    }
    await fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const refetch = useCallback(async (): Promise<void> => {
    if (organizationId === null) {
      return;
    }
    await refetchPages();
  }, [organizationId, refetchPages]);

  const refresh = useCallback(async (): Promise<void> => {
    if (organizationId === null) {
      return;
    }
    const result = await githubApi.listRepositories(organizationId, search(1, true));
    const key = githubQueryKeys.repositories(organizationId, query);
    await queryClient.cancelQueries({ queryKey: key, exact: true });
    queryClient.setQueryData<InfiniteData<GitHubRepositoriesResponse, number>>(key, {
      pages: [result],
      pageParams: [1],
    });
  }, [organizationId, query, search, queryClient]);

  return {
    repositories,
    installations: data?.pages[0]?.installations ?? NO_INSTALLATIONS,
    loading: organizationId !== null && isPending,
    searching: organizationId !== null && isPlaceholderData,
    error: error ? error.message : null,
    hasMore: hasNextPage && !isPlaceholderData,
    loadingMore: isFetchingNextPage,
    loadMore,
    refetch,
    refresh,
  };
}
