import type { PrStatus, Task } from './types';

const WEB_PROTOCOLS: ReadonlySet<string> = new Set(['http:', 'https:']);

const NUMERIC = /^\d+$/;

const NAME = 'PR';

const SEPARATOR = ' · ';

function parse(address: string | null): URL | null {
  if (!address) return null;
  try {
    const parsed = new URL(address);
    return WEB_PROTOCOLS.has(parsed.protocol) ? parsed : null;
  } catch {
    return null;
  }
}

function numberIn(address: URL): string | null {
  const segments = address.pathname.split('/').reverse();
  return segments.find((segment) => NUMERIC.test(segment)) ?? null;
}

export class PullRequest {
  private constructor(
    readonly status: PrStatus | null,
    readonly address: string | null,
    readonly number: string | null
  ) {}

  static of(task: Pick<Task, 'pr_url' | 'pr_status'>): PullRequest | null {
    const parsed = parse(task.pr_url);
    if (!parsed && !task.pr_status) return null;
    return new PullRequest(
      task.pr_status,
      parsed ? task.pr_url : null,
      parsed ? numberIn(parsed) : null
    );
  }

  get label(): string {
    const name = this.number ? `${NAME} #${this.number}` : NAME;
    return this.status ? `${name}${SEPARATOR}${this.status}` : name;
  }
}
