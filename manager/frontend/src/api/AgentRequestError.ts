import type { CodeFailure } from '../features/settings/ai/schemas';

const TRANSIENT_REFUSALS = [408, 429];

export class AgentRequestError extends Error {
  readonly status: number;
  readonly kind: CodeFailure | undefined;

  constructor(message: string, status: number, kind?: CodeFailure) {
    super(message);
    this.name = 'AgentRequestError';
    this.status = status;
    this.kind = kind;
  }

  get retryable(): boolean {
    const refused = this.status >= 400 && this.status < 500;
    return !refused || TRANSIENT_REFUSALS.includes(this.status);
  }
}
