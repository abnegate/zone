import { useQuery } from '@tanstack/react-query';
import { client } from '../../../../api/client';
import { memberLabel } from '../../../settings/memberLabel';
import { retryLoad } from '../../policy/hooks/retry';
import { spendQueryKeys } from '../queryKeys';

export interface SpendTarget {
  id: string;
  label: string;
}

export interface SpendTargets {
  workspaces: SpendTarget[];
  people: SpendTarget[];
}

const NONE: SpendTarget[] = [];

function sorted(targets: SpendTarget[]): SpendTarget[] {
  return [...targets].sort((left, right) => left.label.localeCompare(right.label));
}

export function useSpendTargets(organizationId: string, enabled: boolean): SpendTargets {
  const workspaces = useQuery({
    queryKey: spendQueryKeys.workspaces(organizationId),
    queryFn: async () =>
      sorted(
        (await client.getWorkspaces(organizationId)).map((workspace) => ({
          id: workspace.id,
          label: workspace.name,
        }))
      ),
    enabled,
    retry: retryLoad,
  });

  const people = useQuery({
    queryKey: spendQueryKeys.members(organizationId),
    queryFn: async () =>
      sorted(
        (await client.getOrgMembers(organizationId)).members.map((member) => ({
          id: member.user_id,
          label: memberLabel({
            user_id: member.user_id,
            email: member.email ?? '',
            display_name: member.display_name ?? null,
          }),
        }))
      ),
    enabled,
    retry: retryLoad,
  });

  return { workspaces: workspaces.data ?? NONE, people: people.data ?? NONE };
}
