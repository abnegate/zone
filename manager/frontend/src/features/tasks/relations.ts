import type { Task } from './types';

export interface Relations {
  dependencies: readonly Task[];
  dependents: readonly Task[];
  unknown: number;
}

export const DEPENDS_ON = 'Depends on';

export const BLOCKS = 'Blocks';

export const UNRELATED: Relations = { dependencies: [], dependents: [], unknown: 0 };

function prerequisites(task: Task): string[] {
  return Array.from(new Set(task.dependencies)).filter((id) => id !== task.id);
}

export function relate(tasks: readonly Task[]): ReadonlyMap<string, Relations> {
  const known = new Map(tasks.map((task) => [task.id, task]));
  const dependents = new Map<string, Task[]>();
  for (const task of tasks) {
    for (const id of prerequisites(task)) {
      const blocked = dependents.get(id) ?? [];
      blocked.push(task);
      dependents.set(id, blocked);
    }
  }
  return new Map(
    tasks.map((task) => {
      const ids = prerequisites(task);
      const dependencies = ids.flatMap((id) => known.get(id) ?? []);
      return [
        task.id,
        {
          dependencies,
          dependents: dependents.get(task.id) ?? [],
          unknown: ids.length - dependencies.length,
        },
      ];
    })
  );
}

export function relationsOf(task: Task, tasks: readonly Task[]): Relations {
  const others = tasks.filter((other) => other.id !== task.id);
  return relate([...others, task]).get(task.id) ?? UNRELATED;
}

export function related(relations: Relations): boolean {
  return relations.dependencies.length + relations.dependents.length + relations.unknown > 0;
}
