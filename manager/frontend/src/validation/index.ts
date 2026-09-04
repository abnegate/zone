import type { z } from 'zod';

export * from './schemas';

/**
 * Parse and validate data against a Zod schema.
 * Throws a descriptive error if validation fails.
 */
export function parse<T extends z.ZodType>(schema: T, data: unknown): z.infer<T> {
  const result = schema.safeParse(data);
  if (!result.success) {
    const errors = result.error.issues
      .map((e) => `${e.path.join('.') || 'Field'}: ${e.message}`)
      .join(', ');
    throw new Error(`Validation failed: ${errors}`);
  }
  return result.data;
}

/**
 * Safely parse data against a Zod schema.
 * Returns { success: true, data } or { success: false, error }.
 */
export function safeParse<T extends z.ZodType>(
  schema: T,
  data: unknown
): { success: true; data: z.infer<T> } | { success: false; error: z.ZodError } {
  const result = schema.safeParse(data);
  if (result.success) {
    return { success: true, data: result.data };
  }
  return { success: false, error: result.error };
}

/**
 * Validate data against a Zod schema without throwing.
 * Returns true if valid, false otherwise.
 */
export function isValid<T extends z.ZodType>(schema: T, data: unknown): boolean {
  return schema.safeParse(data).success;
}

/**
 * Get validation errors from a Zod schema.
 * Returns an object mapping field paths to error messages.
 */
export function getErrors<T extends z.ZodType>(schema: T, data: unknown): Record<string, string> {
  const result = schema.safeParse(data);
  if (result.success) {
    return {};
  }
  const errors: Record<string, string> = {};
  for (const error of result.error.issues) {
    const path = error.path.join('.') || '_root';
    errors[path] = error.message;
  }
  return errors;
}
