import type { CodeFailure } from '../features/settings/ai/schemas';

export class AgentRequestError extends Error {
  readonly status: number;
  readonly kind: CodeFailure | undefined;

  constructor(message: string, status: number, kind?: CodeFailure) {
    super(message);
    this.name = 'AgentRequestError';
    this.status = status;
    this.kind = kind;
  }
}
