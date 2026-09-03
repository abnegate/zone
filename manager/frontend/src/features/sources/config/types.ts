/**
 * Source Registry Type Definitions
 *
 * Type definitions for source registry entries. Separated from index.ts
 * to prevent circular dependencies.
 */

import type { SourceCategory, SourceConfig, SourceType } from '../types';

// Form field definition for dynamic form rendering
export interface FormField {
  id: string;
  label: string;
  type: 'text' | 'url' | 'password' | 'number' | 'textarea' | 'toggle';
  placeholder?: string;
  required?: boolean;
  hint?: string;
  defaultValue?: string | number | boolean;
  monospace?: boolean;
  // For toggle fields
  toggleTitle?: string;
  toggleDescription?: string;
}

export interface FormRow {
  fields: FormField[];
}

// Complete source definition - everything needed to render and handle a source type
export interface SourceDefinition {
  id: SourceType;
  name: string;
  category: SourceCategory;
  description: string;
  icon: React.ReactNode;
  badgeColor: string;
  iconWrapperClass: string;
  enabled: boolean;

  // Form configuration
  formFields: (FormField | FormRow)[];
  credentialField?: FormField;
  formHint?: string;

  // Build config from form state
  buildConfig: (state: Record<string, unknown>) => SourceConfig;

  // Generate default name from config
  getDefaultName: (state: Record<string, unknown>) => string;

  // Public URL for this source, when one can be derived from the form
  getUrl?: (state: Record<string, unknown>) => string | undefined;

  // Get field IDs for state initialization
  getFieldIds: () => string[];
}
