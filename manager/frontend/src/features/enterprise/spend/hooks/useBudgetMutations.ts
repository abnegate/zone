import { useMutation, useQueryClient } from '@tanstack/react-query';
import { spendApi } from '../api';
import { spendQueryKeys } from '../queryKeys';
import { SpendRefusal } from '../SpendRefusal';
import type { Budget, CreateBudgetRequest, UpdateBudgetRequest } from '../schemas';

export interface MutationState {
  pending: boolean;
  refusal: SpendRefusal | null;
  error: string | null;
  reset(): void;
}

export interface BudgetMutations {
  create: MutationState & {
    run(request: CreateBudgetRequest, onDone?: (budget: Budget) => void): void;
  };
  update: MutationState & {
    run(budgetId: string, request: UpdateBudgetRequest, onDone?: (budget: Budget) => void): void;
  };
  remove: MutationState & {
    run(budgetId: string, onDone?: () => void): void;
  };
}

interface UpdateVariables {
  budgetId: string;
  request: UpdateBudgetRequest;
}

function stateOf(mutation: {
  isPending: boolean;
  error: Error | null;
  reset(): void;
}): MutationState {
  return {
    pending: mutation.isPending,
    refusal: mutation.error instanceof SpendRefusal ? mutation.error : null,
    error: mutation.error ? mutation.error.message : null,
    reset: mutation.reset,
  };
}

export function useBudgetMutations(organizationId: string): BudgetMutations {
  const queryClient = useQueryClient();
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: spendQueryKeys.budgets(organizationId) });

  const create = useMutation({
    mutationFn: (request: CreateBudgetRequest) => spendApi.createBudget(organizationId, request),
    onSettled: refresh,
  });

  const update = useMutation({
    mutationFn: ({ budgetId, request }: UpdateVariables) =>
      spendApi.updateBudget(organizationId, budgetId, request),
    onSettled: refresh,
  });

  const remove = useMutation({
    mutationFn: (budgetId: string) => spendApi.deleteBudget(organizationId, budgetId),
    onSettled: refresh,
  });

  return {
    create: {
      ...stateOf(create),
      run: (request, onDone) => create.mutate(request, { onSuccess: (budget) => onDone?.(budget) }),
    },
    update: {
      ...stateOf(update),
      run: (budgetId, request, onDone) =>
        update.mutate({ budgetId, request }, { onSuccess: (budget) => onDone?.(budget) }),
    },
    remove: {
      ...stateOf(remove),
      run: (budgetId, onDone) => remove.mutate(budgetId, { onSuccess: () => onDone?.() }),
    },
  };
}
