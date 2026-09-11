/**
 * Reads the serialised shape of a Rust struct out of the server's source.
 *
 * The contract tests assert that the console models what the server sends, and
 * what the server sends is the serialised name — which serde renames. A reader
 * that reports the Rust identifier instead asserts the console against a wire
 * shape the server never emits, and passes while every client fails: the field
 * is dropped, the tolerant parse swallows it, and a parked run shows no card.
 *
 * A `rename_all` on the struct throws rather than being applied, because the
 * console declares snake_case keys and has no mapping to reach for. Add the
 * transform here alongside the console's own the day a server struct needs one.
 */

const ATTRIBUTE = '#[';
const COMMENT = '//';
const SKIP_SERIALIZING_IF = 'skip_serializing_if';
const FIELD = /^(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z_0-9]*)\s*:/;
const RENAME = /\brename\s*=\s*"([^"]+)"/;
const RENAME_ALL = /\brename_all\b/;

export interface RustField {
  name: string;
  optional: boolean;
}

/**
 * The attributes attached to the declaration: everything between the blank line
 * above it and the declaration itself, minus the doc comment. Reading the block
 * rather than the line before it catches an attribute serde spread over several
 * lines, which is where a `rename_all` is most likely to hide.
 */
function attributes(source: string, declaredAt: number): string {
  const preceding = source.slice(0, declaredAt);
  return preceding
    .slice(preceding.lastIndexOf('\n\n') + 1)
    .split('\n')
    .filter((line) => !line.trim().startsWith(COMMENT))
    .join('\n');
}

export function structFields(source: string, structName: string): RustField[] {
  const declaration = new RegExp(`(?:pub\\s+)?struct ${structName}\\s*\\{([^}]*)\\}`).exec(source);
  if (!declaration) throw new Error(`${structName} is not a struct in the Rust source`);
  if (RENAME_ALL.test(attributes(source, declaration.index)))
    throw new Error(`${structName} carries rename_all, which the console does not mirror`);

  const fields: RustField[] = [];
  let renamed: string | undefined;
  let optional = false;
  for (const raw of declaration[1].split('\n')) {
    const line = raw.trim();
    if (line.startsWith(ATTRIBUTE)) {
      renamed = RENAME.exec(line)?.[1] ?? renamed;
      optional ||= line.includes(SKIP_SERIALIZING_IF);
      continue;
    }
    const field = FIELD.exec(line);
    if (!field) continue;
    fields.push({ name: renamed ?? field[1], optional });
    renamed = undefined;
    optional = false;
  }
  if (fields.length === 0) throw new Error(`${structName} declares no fields`);
  return fields;
}
