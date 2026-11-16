import type { z } from 'zod';

import { structFields } from './rust';

export interface WireField {
  optional: boolean;
  nullable: boolean;
}

export type WireShape = Record<string, WireField>;

const NULLABLE_TYPES = new Set(['nullable', 'null']);
const WRAPPER_TYPES = new Set(['optional', 'default', 'prefault', 'catch', 'readonly']);

interface Definition {
  type: string;
  innerType?: z.core.$ZodType;
  in?: z.core.$ZodType;
  options?: readonly z.core.$ZodType[];
}

/**
 * Whether the schema declares null as a value. `isNullable()` would also count a
 * `.catch()`, which swallows a null the server never sends; the declaration is
 * what has to mirror an `Option` on the Rust side.
 */
function declaresNull(schema: z.core.$ZodType): boolean {
  const definition = schema._zod.def as Definition;
  if (NULLABLE_TYPES.has(definition.type)) return true;
  if (definition.options) return definition.options.some(declaresNull);
  if (definition.in) return declaresNull(definition.in);
  if (definition.innerType && WRAPPER_TYPES.has(definition.type)) {
    return declaresNull(definition.innerType);
  }
  return false;
}

export function rustShape(source: string, structName: string): WireShape {
  return Object.fromEntries(
    structFields(source, structName).map(({ name, optional, nullable }) => [
      name,
      { optional, nullable },
    ])
  );
}

/**
 * The shape the console declares, against the one `rustShape` reads.
 * `isOptional()` would also count a `.default()` or `.catch()`, which only make
 * the console tolerant of a field the server always sends; serde omits a key
 * only under `skip_serializing_if`, and that is what the output optionality
 * must mirror. A key serde may omit never travels as null, so it is not
 * compared for one.
 */
export function zodShape(schema: z.ZodObject<z.ZodRawShape>): WireShape {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => {
      const optional = value._zod.optout === 'optional';
      return [key, { optional, nullable: !optional && declaresNull(value) }];
    })
  );
}
