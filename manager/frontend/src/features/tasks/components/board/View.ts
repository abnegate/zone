import type { StageId } from '../../status';

export class View {
  left = 0;
  readonly tops = new Map<StageId, number>();
  opened: string | null = null;
}
