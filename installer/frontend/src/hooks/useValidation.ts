import { useCallback, useState } from 'react';
import { ZodError } from 'zod';
import type { InstallerConfig } from '../types';
import { type StepSchemaKey, StepSchemas } from '../validation/schemas';

export interface ValidationErrors {
  [key: string]: string;
}

export function useValidation() {
  const [errors, setErrors] = useState<ValidationErrors>({});

  const validateStep = useCallback(
    (stepId: StepSchemaKey, config: Partial<InstallerConfig>): boolean => {
      const schema = StepSchemas[stepId];
      if (!schema) return true;

      try {
        schema.parse(config);
        setErrors({});
        return true;
      } catch (error) {
        if (error instanceof ZodError) {
          const newErrors: ValidationErrors = {};
          error.issues.forEach((err) => {
            const path = err.path.join('.');
            if (path) {
              newErrors[path] = err.message;
            }
          });
          setErrors(newErrors);
        }
        return false;
      }
    },
    []
  );

  const clearErrors = useCallback(() => {
    setErrors({});
  }, []);

  const getFieldError = useCallback(
    (field: string): string | undefined => {
      return errors[field];
    },
    [errors]
  );

  const hasErrors = Object.keys(errors).length > 0;

  return {
    errors,
    hasErrors,
    validateStep,
    clearErrors,
    getFieldError,
  };
}
