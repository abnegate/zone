import type { z } from 'zod';

export class ValidationError extends Error {
  readonly issues: readonly z.core.$ZodIssue[];

  constructor(issues: readonly z.core.$ZodIssue[]) {
    const detail = issues
      .map((issue) => `${issue.path.join('.') || 'Field'}: ${issue.message}`)
      .join(', ');
    super(`Validation failed: ${detail}`);
    this.name = 'ValidationError';
    this.issues = issues;
  }
}
