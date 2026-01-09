import { z } from 'zod';
import { getErrors, isValid, parse, safeParse } from './index';

describe('validation utilities', () => {
  const testSchema = z.object({
    name: z.string().min(1, 'Name is required'),
    age: z.number().min(0, 'Age must be positive'),
    email: z.string().email('Invalid email'),
  });

  describe('parse', () => {
    it('returns parsed data when validation succeeds', () => {
      const validData = { name: 'John', age: 30, email: 'john@example.com' };
      const result = parse(testSchema, validData);
      expect(result).toEqual(validData);
    });

    it('throws descriptive error when validation fails', () => {
      const invalidData = { name: '', age: -1, email: 'invalid' };
      expect(() => parse(testSchema, invalidData)).toThrow(/Validation failed/);
    });

    it('includes field paths in error message', () => {
      const invalidData = { name: '', age: 30, email: 'john@example.com' };
      expect(() => parse(testSchema, invalidData)).toThrow(/name/);
    });

    it('handles nested path errors', () => {
      const nestedSchema = z.object({
        user: z.object({
          profile: z.object({
            name: z.string().min(1),
          }),
        }),
      });
      const invalidData = { user: { profile: { name: '' } } };
      expect(() => parse(nestedSchema, invalidData)).toThrow(/user\.profile\.name/);
    });
  });

  describe('safeParse', () => {
    it('returns success true with data when validation succeeds', () => {
      const validData = { name: 'John', age: 30, email: 'john@example.com' };
      const result = safeParse(testSchema, validData);
      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data).toEqual(validData);
      }
    });

    it('returns success false with error when validation fails', () => {
      const invalidData = { name: '', age: -1, email: 'invalid' };
      const result = safeParse(testSchema, invalidData);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error).toBeInstanceOf(z.ZodError);
        expect(result.error.errors.length).toBeGreaterThan(0);
      }
    });
  });

  describe('isValid', () => {
    it('returns true when data is valid', () => {
      const validData = { name: 'John', age: 30, email: 'john@example.com' };
      expect(isValid(testSchema, validData)).toBe(true);
    });

    it('returns false when data is invalid', () => {
      const invalidData = { name: '', age: -1, email: 'invalid' };
      expect(isValid(testSchema, invalidData)).toBe(false);
    });
  });

  describe('getErrors', () => {
    it('returns empty object when validation succeeds', () => {
      const validData = { name: 'John', age: 30, email: 'john@example.com' };
      const errors = getErrors(testSchema, validData);
      expect(errors).toEqual({});
    });

    it('returns field errors when validation fails', () => {
      const invalidData = { name: '', age: -1, email: 'invalid' };
      const errors = getErrors(testSchema, invalidData);
      expect(errors).toHaveProperty('name');
      expect(errors).toHaveProperty('age');
      expect(errors).toHaveProperty('email');
    });

    it('uses _root for root-level errors', () => {
      const stringSchema = z.string().min(1);
      const errors = getErrors(stringSchema, '');
      expect(errors).toHaveProperty('_root');
    });

    it('handles nested field paths', () => {
      const nestedSchema = z.object({
        user: z.object({
          name: z.string().min(1),
        }),
      });
      const invalidData = { user: { name: '' } };
      const errors = getErrors(nestedSchema, invalidData);
      expect('user.name' in errors).toBe(true);
    });
  });
});
